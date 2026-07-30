import type { KeyKind } from '@/lib/bindings'

export const KEY_KIND_BADGE: Record<KeyKind, { short: string, classes: string }> = {
  string: { short: 'str', classes: 'bg-sky-500/10 text-sky-500' },
  list: { short: 'list', classes: 'bg-violet-500/10 text-violet-500' },
  set: { short: 'set', classes: 'bg-amber-500/10 text-amber-500' },
  zset: { short: 'zset', classes: 'bg-rose-500/10 text-rose-500' },
  hash: { short: 'hash', classes: 'bg-emerald-500/10 text-emerald-500' },
  stream: { short: 'strm', classes: 'bg-cyan-500/10 text-cyan-500' },
  other: { short: '?', classes: 'bg-muted text-muted-foreground' },
}

/// Contains-search as a scan pattern: glob specials escaped, wrapped in *.
export function containsPattern(text: string): string {
  if (text === '')
    return ''
  return `*${text.replace(/[\\*?[\]]/g, char => `\\${char}`)}*`
}

/// Compact countdown for key lists: "42s", "5m", "3h", "2d".
export function formatTtl(ms: number): string {
  const seconds = ms / 1000
  if (seconds < 60)
    return `${Math.max(Math.round(seconds), 1)}s`
  if (seconds < 3600)
    return `${Math.round(seconds / 60)}m`
  if (seconds < 86_400)
    return `${Math.round(seconds / 3600)}h`
  return `${Math.round(seconds / 86_400)}d`
}
