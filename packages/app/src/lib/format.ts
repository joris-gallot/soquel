const compact = new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 })

/// Planner estimate: -1 means never analyzed; null when serde hits a non-finite float.
export function formatEstimatedRows(estimate: number | null): string {
  if (estimate === null || estimate < 0)
    return ''
  return compact.format(Math.round(estimate))
}
