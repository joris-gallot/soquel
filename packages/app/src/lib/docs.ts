import type { DocCollectionKind } from '@/lib/bindings'

export const DOC_KIND_BADGE: Record<DocCollectionKind, { short: string, classes: string }> = {
  collection: { short: 'coll', classes: 'bg-sky-500/10 text-sky-500' },
  view: { short: 'view', classes: 'bg-violet-500/10 text-violet-500' },
  timeseries: { short: 'ts', classes: 'bg-emerald-500/10 text-emerald-500' },
  other: { short: '?', classes: 'bg-muted text-muted-foreground' },
}

/// Human label for an extended JSON value: wrapper types unwrapped ($oid hex,
/// $numberLong digits, $date as ISO), documents compacted to {k: v, …}.
function extjsonLabel(value: unknown): string {
  if (value === null || typeof value !== 'object')
    return String(value)
  if (Array.isArray(value))
    return `[${value.map(extjsonLabel).join(', ')}]`
  const entries = Object.entries(value as Record<string, unknown>)
  if (entries.length === 1 && entries[0][0].startsWith('$')) {
    const [key, inner] = entries[0]
    if (key === '$binary') {
      const base64 = (inner as { base64?: string })?.base64 ?? ''
      return `bin(${base64.length > 12 ? `${base64.slice(0, 12)}…` : base64})`
    }
    if (key === '$date') {
      const millis = Number(extjsonLabel(inner))
      return Number.isFinite(millis) ? new Date(millis).toISOString() : String(inner)
    }
    return extjsonLabel(inner)
  }
  return `{${entries.map(([key, entry]) => `${key}: ${extjsonLabel(entry)}`).join(', ')}}`
}

/// Row label for a document's canonical extjson `_id` (DocEntry.id).
export function docIdLabel(id: string | null): string {
  if (id === null)
    return 'no _id'
  try {
    return extjsonLabel(JSON.parse(id))
  }
  catch {
    return id
  }
}

/// One-line preview of a relaxed extjson document, `_id` excluded (the row
/// already leads with it).
export function docPreview(doc: string, max = 140): string {
  try {
    const parsed = JSON.parse(doc) as Record<string, unknown>
    const preview = Object.entries(parsed)
      .filter(([key]) => key !== '_id')
      .map(([key, value]) => `${key}: ${extjsonLabel(value)}`)
      .join('  ')
    return preview === '' ? '(empty document)' : truncate(preview, max)
  }
  catch {
    return truncate(doc, max)
  }
}

function truncate(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, max - 1)}…` : text
}

const COMPACT = new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 })

/// "1.2k", "45m" for collection counts and estimates.
export function compactCount(value: number): string {
  return COMPACT.format(value).toLowerCase()
}

/// Estimates carry a ~ and compact away their false precision; exact counts don't.
export function formatDocCount(count: number, exact: boolean): string {
  const label = count === 1 ? 'doc' : 'docs'
  return exact ? `${count} ${label}` : `~${compactCount(count)} ${label}`
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024)
    return `${Math.round(bytes)} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let value = bytes
  let unit = -1
  do {
    value /= 1024
    unit++
  } while (value >= 1024 && unit < units.length - 1)
  return `${value >= 10 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`
}
