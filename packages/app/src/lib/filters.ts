import type { ColumnFilter, ColumnKind, FilterOp } from '@/lib/bindings'

const COMPARISONS: FilterOp[] = ['eq', 'neq', 'lt', 'lte', 'gt', 'gte']
const TEXTUAL: FilterOp[] = ['contains', 'starts-with']
const NULLNESS: FilterOp[] = ['is-null', 'is-not-null']

export const FILTER_OPS_BY_KIND: Record<ColumnKind, FilterOp[]> = {
  'bool': ['eq', 'neq', ...NULLNESS],
  'number': [...COMPARISONS, ...NULLNESS],
  'text': ['eq', 'neq', ...TEXTUAL, ...NULLNESS],
  'json': [...TEXTUAL, ...NULLNESS],
  'bytes': NULLNESS,
  'date-time': [...COMPARISONS, ...NULLNESS],
  'uuid': ['eq', 'neq', ...NULLNESS],
  'array': [...TEXTUAL, ...NULLNESS],
  'other': ['eq', 'neq', ...TEXTUAL, ...NULLNESS],
}

export const FILTER_OP_LABELS: Record<FilterOp, string> = {
  'eq': '=',
  'neq': '≠',
  'lt': '<',
  'lte': '≤',
  'gt': '>',
  'gte': '≥',
  'contains': 'contains',
  'starts-with': 'starts with',
  'is-null': 'is null',
  'is-not-null': 'is not null',
}

export const OP_NEEDS_VALUE: Record<FilterOp, boolean> = {
  'eq': true,
  'neq': true,
  'lt': true,
  'lte': true,
  'gt': true,
  'gte': true,
  'contains': true,
  'starts-with': true,
  'is-null': false,
  'is-not-null': false,
}

export function filterLabel(filter: ColumnFilter): string {
  const op = FILTER_OP_LABELS[filter.op]
  return OP_NEEDS_VALUE[filter.op]
    ? `${filter.column} ${op} ${filter.value ?? ''}`
    : `${filter.column} ${op}`
}
