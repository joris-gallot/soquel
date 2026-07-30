import { describe, expect, it } from 'vitest'
import costOnlyRaw from '@/lib/__fixtures__/explain-cost-only.json?raw'
import joinSortRaw from '@/lib/__fixtures__/explain-join-sort.json?raw'
import parallelRaw from '@/lib/__fixtures__/explain-parallel.json?raw'
import subplanRaw from '@/lib/__fixtures__/explain-subplan.json?raw'
import { flattenPlan, formatMs, hiddenByCollapse, parseExplain } from '@/lib/explain'

function statementWith(json: unknown) {
  return {
    columns: [{ name: 'QUERY PLAN', dataType: 'jsonb', kind: 'json' as const }],
    rows: [[JSON.stringify(json)]],
  }
}

const ANALYZED = [
  {
    'Planning Time': 0.2,
    'Execution Time': 10,
    'Plan': {
      'Node Type': 'Hash Join',
      'Join Type': 'Inner',
      'Hash Cond': '(o.customer_id = c.id)',
      'Total Cost': 100,
      'Plan Rows': 1000,
      'Actual Rows': 900,
      'Actual Loops': 1,
      'Actual Total Time': 9,
      'Plans': [
        {
          'Node Type': 'Seq Scan',
          'Relation Name': 'orders',
          'Schema': 'app',
          'Alias': 'o',
          'Filter': '(amount > 0)',
          'Total Cost': 60,
          'Plan Rows': 10,
          'Actual Rows': 5000,
          'Actual Loops': 1,
          'Actual Total Time': 6,
        },
        {
          'Node Type': 'Hash',
          'Total Cost': 30,
          'Plan Rows': 100,
          'Actual Rows': 0,
          'Actual Loops': 0,
          'Actual Total Time': 0,
          'Plans': [
            {
              'Node Type': 'Index Only Scan',
              'Relation Name': 'customers',
              'Schema': 'app',
              'Alias': 'customers',
              'Index Name': 'customers_pkey',
              'Total Cost': 20,
              'Plan Rows': 100,
              'Actual Rows': 100,
              'Actual Loops': 2,
              'Actual Total Time': 1,
            },
          ],
        },
      ],
    },
  },
]

describe('parseExplain', () => {
  it('rejects non-explain statements and text-format plans', () => {
    expect(parseExplain({ columns: [], rows: [] })).toBeNull()
    expect(
      parseExplain({
        columns: [{ name: 'QUERY PLAN', dataType: 'text', kind: 'text' }],
        rows: [['Seq Scan on orders  (cost=0.00..1.00 rows=1 width=4)']],
      }),
    ).toBeNull()
    expect(
      parseExplain({
        columns: [{ name: 'id', dataType: 'int4', kind: 'number' }],
        rows: [['1']],
      }),
    ).toBeNull()
  })

  it('parses an analyzed plan with loop-adjusted exclusive times', () => {
    const plans = parseExplain(statementWith(ANALYZED))!
    expect(plans).toHaveLength(1)
    const { root, planningMs, executionMs, analyzed } = plans[0]
    expect(analyzed).toBe(true)
    expect(planningMs).toBe(0.2)
    expect(executionMs).toBe(10)

    expect(root.nodeType).toBe('Hash Join')
    expect(root.condition).toBe('Hash Cond: (o.customer_id = c.id)')
    // 9 - (6 + 0) = 3; the index scan (1ms x 2 loops) nests under Hash.
    expect(root.exclusiveMs).toBe(3)

    const [scan, hash] = root.children
    expect(scan.target).toBe('on app.orders o')
    expect(scan.condition).toBe('Filter: (amount > 0)')
    // 10 estimated vs 5000 actual: off by 10x or more.
    expect(scan.estimateOff).toBe(true)
    expect(scan.heat).toBeCloseTo(6 / 9)

    expect(hash.neverExecuted).toBe(true)
    expect(hash.estimateOff).toBe(false)
    const index = hash.children[0]
    expect(index.target).toBe('on app.customers using customers_pkey')
    expect(index.inclusiveMs).toBe(2)
    expect(index.id).toBe('0.1.0')
  })

  it('falls back to cost heat without analyze', () => {
    const plans = parseExplain(
      statementWith([
        {
          Plan: {
            'Node Type': 'Seq Scan',
            'Relation Name': 'events',
            'Total Cost': 200,
            'Plan Rows': 10000,
          },
        },
      ]),
    )!
    const { root, analyzed } = plans[0]
    expect(analyzed).toBe(false)
    expect(root.inclusiveMs).toBeNull()
    expect(root.heat).toBe(1)
    expect(root.estimateOff).toBe(false)
  })

  it('joins multi-row json output before parsing', () => {
    const pretty = JSON.stringify(ANALYZED, null, 2).split('\n')
    const plans = parseExplain({
      columns: [{ name: 'QUERY PLAN', dataType: null, kind: 'other' }],
      rows: pretty.map(line => [line]),
    })
    expect(plans).not.toBeNull()
    expect(plans![0].root.nodeType).toBe('Hash Join')
  })
})

// Captured from the seeded test postgres (EXPLAIN ... FORMAT JSON via psql):
// the real field names and shapes, not hand-written approximations.
describe('parseExplain on real postgres output', () => {
  // psql delivers the json pretty-printed: feed it line by line like the driver does.
  function realStatement(raw: string) {
    return {
      columns: [{ name: 'QUERY PLAN', dataType: 'jsonb', kind: 'json' as const }],
      rows: raw.trimEnd().split('\n').map(line => [line]),
    }
  }

  function invariants(raw: string) {
    const plans = parseExplain(realStatement(raw))
    expect(plans).not.toBeNull()
    const nodes = plans!.flatMap(plan => flattenPlan(plan.root))
    const ids = new Set(nodes.map(node => node.id))
    expect(ids.size).toBe(nodes.length)
    for (const node of nodes) {
      expect(node.heat).toBeGreaterThanOrEqual(0)
      expect(node.heat).toBeLessThanOrEqual(1)
      expect(node.exclusiveCost).toBeGreaterThanOrEqual(0)
      if (node.exclusiveMs !== null) {
        expect(Number.isFinite(node.exclusiveMs)).toBe(true)
        expect(node.exclusiveMs).toBeGreaterThanOrEqual(0)
      }
    }
    return { plans: plans!, nodes }
  }

  it('handles a join + aggregate + sort analyze plan', () => {
    const { plans, nodes } = invariants(joinSortRaw)
    expect(plans[0].analyzed).toBe(true)
    expect(plans[0].executionMs).not.toBeNull()
    expect(nodes.map(node => node.nodeType)).toContain('Hash Join')
    // No VERBOSE: postgres omits Schema, targets are unqualified.
    const scan = nodes.find(node => node.target === 'on orders o')
    expect(scan).toBeDefined()
  })

  it('handles a parallel Gather plan', () => {
    const { nodes } = invariants(parallelRaw)
    const gather = nodes.find(node => node.nodeType === 'Gather')
    expect(gather).toBeDefined()
    expect(gather!.children.length).toBeGreaterThan(0)
  })

  it('handles CTE and InitPlan children', () => {
    const { nodes } = invariants(subplanRaw)
    expect(nodes.map(node => node.nodeType)).toContain('CTE Scan')
    // The InitPlan rides along as a child: more nodes than the outer chain alone.
    expect(nodes.length).toBeGreaterThanOrEqual(4)
  })

  it('handles a cost-only plan', () => {
    const { plans, nodes } = invariants(costOnlyRaw)
    expect(plans[0].analyzed).toBe(false)
    expect(plans[0].executionMs).toBeNull()
    expect(nodes.every(node => node.inclusiveMs === null)).toBe(true)
    expect(plans[0].root.heat).toBeGreaterThan(0)
  })
})

describe('hiddenByCollapse', () => {
  it('hides descendants only, and never trips on sibling prefixes', () => {
    const collapsed = new Set(['0.1'])
    expect(hiddenByCollapse('0.1.0', collapsed)).toBe(true)
    expect(hiddenByCollapse('0.1.0.2', collapsed)).toBe(true)
    // The collapsed node itself stays visible.
    expect(hiddenByCollapse('0.1', collapsed)).toBe(false)
    // "0.10" shares the string prefix but is a sibling, not a child.
    expect(hiddenByCollapse('0.10', collapsed)).toBe(false)
    expect(hiddenByCollapse('0.0', new Set())).toBe(false)
  })
})

describe('flattenPlan', () => {
  it('yields pre-order rows with hierarchical ids', () => {
    const plans = parseExplain(statementWith(ANALYZED))!
    const rows = flattenPlan(plans[0].root)
    expect(rows.map(r => r.id)).toEqual(['0', '0.0', '0.1', '0.1.0'])
    expect(rows.map(r => r.depth)).toEqual([0, 1, 1, 2])
  })
})

describe('formatMs', () => {
  it('scales precision with magnitude', () => {
    expect(formatMs(0.056)).toBe('0.06ms')
    expect(formatMs(12.34)).toBe('12.3ms')
    expect(formatMs(456.7)).toBe('457ms')
    expect(formatMs(1234)).toBe('1.23s')
  })
})
