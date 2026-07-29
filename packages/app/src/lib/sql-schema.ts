import type { SQLNamespace } from '@codemirror/lang-sql'
import type { SchemaSnapshot } from '@/lib/bindings'

export const DEFAULT_SCHEMA = 'public'

/// Feed the introspection snapshot to lang-sql completion: schema.table.column.
export function snapshotToNamespace(snapshot: SchemaSnapshot): SQLNamespace {
  const namespace: Record<string, Record<string, string[]>> = {}
  for (const schema of snapshot.schemas) {
    namespace[schema.name] = Object.fromEntries(
      schema.tables.map(table => [table.name, table.columns.map(column => column.name)]),
    )
  }
  return namespace
}
