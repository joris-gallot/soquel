import type { DuplicateStrategy, ImportPreview, PreviewEntry } from '@/lib/bindings'
import { open, save } from '@tauri-apps/plugin-dialog'

/// Own extension so the OS can associate it later: Windows and macOS bind the
/// last one, which rules out a `.soquel.json` pair.
const EXPORT_FILTERS = [{ name: 'Soquel connections', extensions: ['soquel'] }]
/// The file was JSON-named before it had an extension of its own.
const IMPORT_FILTERS = [{ name: 'Soquel connections', extensions: ['soquel', 'json'] }]

export const DEFAULT_EXPORT_NAME = 'connections.soquel'

export const DUPLICATE_STRATEGIES = ['skip', 'replace', 'keep-both'] as const satisfies readonly DuplicateStrategy[]

export const DUPLICATE_STRATEGY_LABELS: Record<DuplicateStrategy, { label: string, hint: string }> = {
  'skip': { label: 'Skip them', hint: 'Keep what is already here, ignore the file version.' },
  'replace': { label: 'Replace them', hint: 'Overwrite the existing entry, its password included.' },
  'keep-both': { label: 'Keep both', hint: 'Import a second copy under a suffixed name.' },
}

/** Null when the user cancels the dialog. */
export function pickImportFile(): Promise<string | null> {
  return open({ multiple: false, directory: false, filters: IMPORT_FILTERS }) as Promise<string | null>
}

export function pickExportPath(): Promise<string | null> {
  return save({ defaultPath: DEFAULT_EXPORT_NAME, filters: EXPORT_FILTERS })
}

/// A passphrase is only worth asking for when it can actually be re-typed right.
export function passphraseIssue(passphrase: string, confirmation: string): string | null {
  if (passphrase.length < 8)
    return 'Use at least 8 characters.'
  if (passphrase !== confirmation)
    return 'The two passphrases do not match.'
  return null
}

export interface PlanEntry extends PreviewEntry {
  kind: 'connection' | 'tunnel'
}

export interface ImportPlan {
  entries: PlanEntry[]
  duplicates: number
  problems: PlanEntry[]
  secrets: number
  commands: number
}

/// Connections and tunnels read as one list: the counts the dialog announces
/// are about the file as a whole.
export function importPlan(preview: ImportPreview): ImportPlan {
  const entries: PlanEntry[] = [
    ...preview.connections.map(entry => ({ ...entry, kind: 'connection' as const })),
    ...preview.tunnels.map(entry => ({ ...entry, kind: 'tunnel' as const })),
  ]
  return {
    entries,
    duplicates: entries.filter(entry => entry.duplicate).length,
    problems: entries.filter(entry => entry.problem !== null),
    secrets: entries.filter(entry => entry.hasSecret).length,
    commands: entries.filter(entry => entry.hasCommand).length,
  }
}

export function exportSummaryMessage(counts: { connections: number, tunnels: number, secrets: number }): string {
  const parts = [plural(counts.connections, 'connection')]
  if (counts.tunnels > 0)
    parts.push(plural(counts.tunnels, 'tunnel'))
  if (counts.secrets > 0)
    parts.push(plural(counts.secrets, 'password'))
  return `Exported ${parts.join(', ')}`
}

export function importOutcomeMessage(outcome: { created: number, replaced: number, skipped: number, tunnelsCreated: number }): string {
  const parts: string[] = []
  if (outcome.created > 0)
    parts.push(`${outcome.created} added`)
  if (outcome.replaced > 0)
    parts.push(`${outcome.replaced} replaced`)
  if (outcome.skipped > 0)
    parts.push(`${outcome.skipped} skipped`)
  if (outcome.tunnelsCreated > 0)
    parts.push(plural(outcome.tunnelsCreated, 'tunnel'))
  return parts.length === 0 ? 'Nothing to import' : `Imported: ${parts.join(', ')}`
}

function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? '' : 's'}`
}
