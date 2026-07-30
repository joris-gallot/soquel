import type { ConnectorKind, StatementResult } from '@/lib/bindings'

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

/// The dialect-specific SQL that produces a tree-renderable plan.
export function explainSql(kind: ConnectorKind, analyze: boolean, sql: string): string {
  if (kind === 'mysql')
    return analyze ? `EXPLAIN ANALYZE ${sql}` : `EXPLAIN FORMAT=JSON ${sql}`
  return `EXPLAIN (${analyze ? 'ANALYZE, FORMAT JSON' : 'FORMAT JSON'}) ${sql}`
}

/// mysql's EXPLAIN ANALYZE speaks TREE text (no JSON on 8.0): render it as-is.
export function explainTreeText(
  statement: Pick<StatementResult, 'columns' | 'rows'>,
): string | null {
  if (statement.columns.length !== 1 || statement.columns[0].name !== 'EXPLAIN')
    return null
  const text = statement.rows.map(row => row[0] ?? '').join('\n').trim()
  return text.startsWith('->') ? text : null
}

/// Null unless the statement is a json plan result set (pg or mysql).
export function parseExplain(
  statement: Pick<StatementResult, 'columns' | 'rows'>,
): ExplainPlan[] | null {
  if (statement.columns.length !== 1)
    return null
  if (statement.columns[0].name === 'EXPLAIN')
    return parseMysqlExplain(statement)
  if (statement.columns[0].name !== 'QUERY PLAN')
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

/// Ancestor check is a prefix check on the hierarchical ids; the collapsed
/// node itself stays visible ("0.1" hides "0.1.0" but not "0.10").
export function hiddenByCollapse(id: string, collapsed: ReadonlySet<string>): boolean {
  for (const ancestor of collapsed) {
    if (id.startsWith(`${ancestor}.`))
      return true
  }
  return false
}

// -------- mysql: query_block / nested_loop / table shapes --------

const MYSQL_WRAPPERS: Record<string, string> = {
  ordering_operation: 'Ordering',
  grouping_operation: 'Grouping',
  duplicates_removal: 'Distinct',
  windowing: 'Windowing',
  buffer_result: 'Buffer',
}

const MYSQL_ACCESS_TYPES: Record<string, string> = {
  ALL: 'Table scan',
  index: 'Index scan',
  range: 'Range scan',
  ref: 'Ref lookup',
  eq_ref: 'Unique lookup',
  const: 'Const row',
  system: 'Const row',
  fulltext: 'Fulltext search',
  index_merge: 'Index merge',
  unique_subquery: 'Unique subquery',
  index_subquery: 'Index subquery',
}

function parseMysqlExplain(
  statement: Pick<StatementResult, 'columns' | 'rows'>,
): ExplainPlan[] | null {
  const text = statement.rows.map(row => row[0] ?? '').join('\n').trim()
  if (!text.startsWith('{'))
    return null
  let parsed: unknown
  try {
    parsed = JSON.parse(text)
  }
  catch {
    return null
  }
  const block = (parsed as RawNode)?.query_block as RawNode | undefined
  if (!block)
    return null

  const root = mysqlQueryBlock(block)
  finalizeMysqlPlan(root, '0', 0)
  const basis = root.inclusiveCost > 0 ? root.inclusiveCost : 1
  applyHeat(root, basis, false)
  // FORMAT=JSON carries estimates only; ANALYZE speaks TREE text instead.
  return [{ root, planningMs: null, executionMs: null, analyzed: false }]
}

function mysqlQueryBlock(block: RawNode): PlanNode {
  const node = mysqlNode('Query block', block)
  node.inclusiveCost = mysqlCost(block, 'query_cost') ?? 0
  node.totalCost = node.inclusiveCost
  return node
}

function mysqlNode(nodeType: string, raw: RawNode): PlanNode {
  const children = mysqlChildren(raw)
  const flags = [
    raw.using_filesort === true ? 'using filesort' : null,
    raw.using_temporary_table === true ? 'using temporary' : null,
  ].filter((flag): flag is string => flag !== null)
  const cost = mysqlCost(raw, 'prefix_cost') ?? mysqlCost(raw, 'query_cost')
  const inclusiveCost
    = cost ?? children.reduce((total, child) => Math.max(total, child.inclusiveCost), 0)
  return {
    id: '',
    depth: 0,
    nodeType,
    target: mysqlTarget(raw),
    condition:
      typeof raw.attached_condition === 'string'
        ? `Filter: ${raw.attached_condition}`
        : flags.length > 0
          ? flags.join(', ')
          : null,
    totalCost: inclusiveCost,
    planRows: numberOrNull(raw.rows_examined_per_scan) ?? 0,
    actualRows: null,
    actualLoops: null,
    inclusiveMs: null,
    exclusiveMs: null,
    inclusiveCost,
    exclusiveCost: 0,
    heat: 0,
    estimateOff: false,
    neverExecuted: false,
    children,
  }
}

function mysqlChildren(raw: RawNode): PlanNode[] {
  const children: PlanNode[] = []
  for (const [key, label] of Object.entries(MYSQL_WRAPPERS)) {
    const wrapped = raw[key]
    if (wrapped && typeof wrapped === 'object')
      children.push(mysqlNode(label, wrapped as RawNode))
  }
  if (Array.isArray(raw.nested_loop)) {
    const tables = (raw.nested_loop as RawNode[])
      .map(entry => entry.table)
      .filter((table): table is RawNode => typeof table === 'object' && table !== null)
      .map(mysqlTable)
    // prefix_cost is cumulative across join siblings: displayed cost keeps the
    // prefix, but the heat math needs each table's own share (the delta).
    let previousPrefix = 0
    for (const table of tables) {
      const prefix = table.inclusiveCost
      table.inclusiveCost = Math.max(prefix - previousPrefix, 0)
      previousPrefix = Math.max(prefix, previousPrefix)
    }
    const loop = mysqlNode('Nested loop', {})
    loop.children = tables
    loop.inclusiveCost = previousPrefix
    loop.totalCost = previousPrefix
    children.push(loop)
  }
  if (raw.table && typeof raw.table === 'object')
    children.push(mysqlTable(raw.table as RawNode))
  if (Array.isArray(raw.attached_subqueries)) {
    for (const entry of raw.attached_subqueries as RawNode[]) {
      const block = entry.query_block
      if (block && typeof block === 'object') {
        const subquery = mysqlNode('Subquery', {})
        subquery.children = [mysqlQueryBlock(block as RawNode)]
        subquery.inclusiveCost = subquery.children[0].inclusiveCost
        subquery.totalCost = subquery.inclusiveCost
        children.push(subquery)
      }
    }
  }
  const materialized = raw.materialized_from_subquery as RawNode | undefined
  if (materialized?.query_block && typeof materialized.query_block === 'object')
    children.push(mysqlQueryBlock(materialized.query_block as RawNode))
  const union = raw.union_result as RawNode | undefined
  if (union && typeof union === 'object') {
    const branches = (Array.isArray(union.query_specifications) ? union.query_specifications as RawNode[] : [])
      .map(entry => entry.query_block)
      .filter((block): block is RawNode => typeof block === 'object' && block !== null)
      .map(mysqlQueryBlock)
    const result = mysqlNode('Union', union)
    result.children = branches
    result.inclusiveCost = branches.reduce((total, branch) => total + branch.inclusiveCost, 0)
    result.totalCost = result.inclusiveCost
    children.push(result)
  }
  return children
}

function mysqlTable(raw: RawNode): PlanNode {
  const access = typeof raw.access_type === 'string' ? raw.access_type : ''
  return mysqlNode(MYSQL_ACCESS_TYPES[access] ?? `Access (${access || 'unknown'})`, raw)
}

function mysqlTarget(raw: RawNode): string | null {
  const parts: string[] = []
  if (typeof raw.table_name === 'string')
    parts.push(`on ${raw.table_name}`)
  if (typeof raw.key === 'string')
    parts.push(`using ${raw.key}`)
  return parts.length > 0 ? parts.join(' ') : null
}

function mysqlCost(raw: RawNode, key: string): number | null {
  const info = raw.cost_info as RawNode | undefined
  const value = info?.[key]
  if (typeof value === 'string' && value !== '')
    return Number.isFinite(Number(value)) ? Number(value) : null
  return numberOrNull(value)
}

/// mysql costs are cumulative (prefix_cost): same inclusive/exclusive math as pg.
function finalizeMysqlPlan(node: PlanNode, id: string, depth: number) {
  node.id = id
  node.depth = depth
  const childrenCost = node.children.reduce((total, child) => total + child.inclusiveCost, 0)
  node.exclusiveCost = Math.max(node.inclusiveCost - childrenCost, 0)
  node.children.forEach((child, index) => finalizeMysqlPlan(child, `${id}.${index}`, depth + 1))
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
