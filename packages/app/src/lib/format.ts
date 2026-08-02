const compact = new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 })

/// UTC, and the month spelled out: the licence window is signed in UTC, and a
/// numeric 1/12 is two different days either side of the Atlantic.
const day = new Intl.DateTimeFormat('en', {
  day: 'numeric',
  month: 'long',
  year: 'numeric',
  timeZone: 'UTC',
})

export function formatDay(raw: string | null | undefined): string | null {
  if (!raw)
    return null
  const date = new Date(raw)
  return Number.isNaN(date.getTime()) ? null : day.format(date)
}

/// Planner estimate: -1 means never analyzed; null when serde hits a non-finite float.
export function formatEstimatedRows(estimate: number | null): string {
  if (estimate === null || estimate < 0)
    return ''
  return compact.format(Math.round(estimate))
}
