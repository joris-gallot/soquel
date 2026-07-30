import type { StatementResult } from '@/lib/bindings'

export interface PlanNode {
  /** Hierarchical path ("0", "0.1", …): ancestor checks are prefix checks. */
  id: string
  depth: number
  nodeType: string
  /** "on app.events e", "using orders_pkey", … */
  target: string | null
  condition: string | null
  totalCost: number
  planRows: number
  /** Per-loop averages; null without ANALYZE. */
  actualRows: number | null
  actualLoops: number | null
  /** Loop-adjusted totals. */
  inclusiveMs: number | null
  exclusiveMs: number | null
  inclusiveCost: number
  exclusiveCost: number
  /** Exclusive share of the root total (time when analyzed, cost otherwise), 0..1. */
  heat: number
  /** Planner estimate off by >= 10x versus actual rows. */
  estimateOff: boolean
  neverExecuted: boolean
  children: PlanNode[]
}

export interface ExplainPlan {
  root: PlanNode
  planningMs: number | null
  executionMs: number | null
  analyzed: boolean
}

const CONDITION_KEYS = [
  'Index Cond',
  'Recheck Cond',
  'Hash Cond',
  'Merge Cond',
  'Join Filter',
  'Filter',
  'Sort Key',
  'Group Key',
] as const

type RawNode = Record<string, unknown>

/// Null unless the statement is an EXPLAIN (FORMAT JSON) result set.
export function parseExplain(
  statement: Pick<StatementResult, 'columns' | 'rows'>,
): ExplainPlan[] | null {
  if (statement.columns.length !== 1 || statement.columns[0].name !== 'QUERY PLAN')
    return null
  const text = statement.rows.map(row => row[0] ?? '').join('\n').trim()
  if (!text.startsWith('['))
    return null
  let entries: unknown
  try {
    entries = JSON.parse(text)
  }
  catch {
    return null
  }
  if (!Array.isArray(entries))
    return null

  const plans: ExplainPlan[] = []
  for (const entry of entries) {
    const raw = (entry as RawNode)?.Plan as RawNode | undefined
    if (!raw)
      return null
    const analyzed = typeof raw['Actual Total Time'] === 'number'
    const root = buildNode(raw, '0', 0)
    const basis = analyzed ? root.inclusiveMs ?? 0 : root.inclusiveCost
    applyHeat(root, basis > 0 ? basis : 1, analyzed)
    plans.push({
      root,
      planningMs: numberOrNull((entry as RawNode)['Planning Time']),
      executionMs: numberOrNull((entry as RawNode)['Execution Time']),
      analyzed,
    })
  }
  return plans.length > 0 ? plans : null
}

/// Pre-order flatten for flat rendering with indent guides.
export function flattenPlan(root: PlanNode): PlanNode[] {
  const rows: PlanNode[] = []
  const walk = (node: PlanNode) => {
    rows.push(node)
    node.children.forEach(walk)
  }
  walk(root)
  return rows
}

function buildNode(raw: RawNode, id: string, depth: number): PlanNode {
  const children = (Array.isArray(raw.Plans) ? (raw.Plans as RawNode[]) : []).map(
    (child, index) => buildNode(child, `${id}.${index}`, depth + 1),
  )

  const loops = numberOrNull(raw['Actual Loops'])
  const perLoopMs = numberOrNull(raw['Actual Total Time'])
  const inclusiveMs = perLoopMs === null ? null : perLoopMs * (loops ?? 1)
  const childrenMs = children.reduce((total, child) => total + (child.inclusiveMs ?? 0), 0)
  const inclusiveCost = numberOrNull(raw['Total Cost']) ?? 0
  const childrenCost = children.reduce((total, child) => total + child.inclusiveCost, 0)

  const planRows = numberOrNull(raw['Plan Rows']) ?? 0
  const actualRows = numberOrNull(raw['Actual Rows'])
  const neverExecuted = loops === 0
  const estimateOff
    = actualRows !== null
      && !neverExecuted
      && offByTenfold(Math.max(planRows, 1), Math.max(actualRows, 1))

  return {
    id,
    depth,
    nodeType: String(raw['Node Type'] ?? 'Unknown'),
    target: target(raw),
    condition: condition(raw),
    totalCost: inclusiveCost,
    planRows,
    actualRows,
    actualLoops: loops,
    inclusiveMs,
    exclusiveMs: inclusiveMs === null ? null : Math.max(inclusiveMs - childrenMs, 0),
    inclusiveCost,
    exclusiveCost: Math.max(inclusiveCost - childrenCost, 0),
    heat: 0,
    estimateOff,
    neverExecuted,
    children,
  }
}

function applyHeat(node: PlanNode, basis: number, analyzed: boolean) {
  const exclusive = analyzed ? node.exclusiveMs ?? 0 : node.exclusiveCost
  node.heat = Math.min(exclusive / basis, 1)
  node.children.forEach(child => applyHeat(child, basis, analyzed))
}

function target(raw: RawNode): string | null {
  const parts: string[] = []
  const relation = raw['Relation Name'] ?? raw['CTE Name'] ?? raw['Function Name']
  if (typeof relation === 'string') {
    const schema = raw.Schema
    const qualified = typeof schema === 'string' ? `${schema}.${relation}` : relation
    const alias = raw.Alias
    parts.push(
      `on ${qualified}${typeof alias === 'string' && alias !== relation ? ` ${alias}` : ''}`,
    )
  }
  if (typeof raw['Index Name'] === 'string')
    parts.push(`using ${raw['Index Name']}`)
  if (typeof raw['Join Type'] === 'string' && raw['Join Type'] !== 'Inner')
    parts.unshift(String(raw['Join Type']).toLowerCase())
  return parts.length > 0 ? parts.join(' ') : null
}

function condition(raw: RawNode): string | null {
  for (const key of CONDITION_KEYS) {
    const value = raw[key]
    if (typeof value === 'string')
      return `${key}: ${value}`
    if (Array.isArray(value) && value.length > 0)
      return `${key}: ${value.join(', ')}`
  }
  return null
}

function offByTenfold(a: number, b: number): boolean {
  return Math.max(a, b) / Math.min(a, b) >= 10
}

function numberOrNull(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

export function formatMs(ms: number): string {
  if (ms >= 1000)
    return `${(ms / 1000).toFixed(2)}s`
  if (ms >= 100)
    return `${ms.toFixed(0)}ms`
  if (ms >= 10)
    return `${ms.toFixed(1)}ms`
  return `${ms.toFixed(2)}ms`
}

const compact = new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 })

export function formatRows(rows: number): string {
  return compact.format(Math.round(rows))
}
