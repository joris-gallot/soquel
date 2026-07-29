export interface HistoryEntry {
  sql: string
  at: number
  durationMs: number
  ok: boolean
}

const CAP = 100

/// Newest first; consecutive reruns of the same sql collapse into one entry.
export function pushHistory(entries: HistoryEntry[], entry: HistoryEntry): HistoryEntry[] {
  const rest = entries[0]?.sql === entry.sql ? entries.slice(1) : entries
  return [entry, ...rest].slice(0, CAP)
}

export function filterHistory(entries: HistoryEntry[], query: string): HistoryEntry[] {
  const needle = query.trim().toLowerCase()
  if (needle === '')
    return entries
  return entries.filter(entry => entry.sql.toLowerCase().includes(needle))
}
