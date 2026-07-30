import type { ExportFormat } from '@/lib/bindings'
import { save } from '@tauri-apps/plugin-dialog'

export const EXPORT_FORMATS: Record<ExportFormat, { label: string, extension: string }> = {
  csv: { label: 'CSV', extension: 'csv' },
  json: { label: 'JSON', extension: 'json' },
  sql: { label: 'SQL inserts', extension: 'sql' },
  markdown: { label: 'Markdown', extension: 'md' },
}

export const EXPORT_FORMAT_KEYS = Object.keys(EXPORT_FORMATS) as ExportFormat[]

/** Null when the user cancels the dialog. */
export function pickExportPath(format: ExportFormat, baseName: string): Promise<string | null> {
  const { label, extension } = EXPORT_FORMATS[format]
  return save({
    defaultPath: `${baseName}.${extension}`,
    filters: [{ name: label, extensions: [extension] }],
  })
}
