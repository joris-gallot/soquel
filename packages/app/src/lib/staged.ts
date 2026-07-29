import type { QueryColumn, TableChanges } from '@/lib/bindings'

/// Cell values: string = provided, null = NULL; an absent insert key = column DEFAULT.
export interface StagedChanges {
  edits: Record<number, Record<string, string | null>>
  deletes: number[]
  inserts: Array<Record<string, string | null>>
}

export function emptyStaged(): StagedChanges {
  return { edits: {}, deletes: [], inserts: [] }
}

export function stagedCount(staged: StagedChanges): number {
  const edits = Object.keys(staged.edits)
    .filter(row => !staged.deletes.includes(Number(row)))
    .length
  return edits + staged.deletes.length + staged.inserts.length
}

export function buildTableChanges(
  staged: StagedChanges,
  rows: (string | null)[][],
  columns: QueryColumn[],
  keyColumns: string[],
  schema: string,
  table: string,
): TableChanges {
  const indexOf = new Map(columns.map((column, index) => [column.name, index]))
  // Keys always carry the ORIGINAL row values, even when the key column itself was edited.
  const keyOf = (row: number) =>
    keyColumns.map(column => ({ column, value: rows[row]?.[indexOf.get(column) ?? -1] ?? null }))

  return {
    schema,
    table,
    updates: Object.entries(staged.edits)
      .filter(([row]) => !staged.deletes.includes(Number(row)))
      .map(([row, cells]) => ({
        key: keyOf(Number(row)),
        set: Object.entries(cells).map(([column, value]) => ({ column, value })),
      })),
    deletes: staged.deletes.map(row => ({ key: keyOf(row) })),
    inserts: staged.inserts.map(values => ({
      values: Object.entries(values).map(([column, value]) => ({ column, value })),
    })),
  }
}

function literal(value: string | null): string {
  return value === null ? 'NULL' : `'${value.replaceAll('\'', '\'\'')}'`
}

function ident(name: string): string {
  return `"${name.replaceAll('"', '""')}"`
}

/// Display only: execution binds every value as a parameter.
export function previewSql(changes: TableChanges): string[] {
  const target = `${ident(changes.schema)}.${ident(changes.table)}`
  const where = (key: { column: string, value: string | null }[]) =>
    key
      .map(cell => cell.value === null
        ? `${ident(cell.column)} IS NULL`
        : `${ident(cell.column)} = ${literal(cell.value)}`)
      .join(' AND ')

  return [
    ...changes.updates.map(update =>
      `UPDATE ${target} SET ${update.set.map(cell => `${ident(cell.column)} = ${literal(cell.value)}`).join(', ')} WHERE ${where(update.key)};`),
    ...changes.deletes.map(remove => `DELETE FROM ${target} WHERE ${where(remove.key)};`),
    ...changes.inserts.map(insert =>
      insert.values.length === 0
        ? `INSERT INTO ${target} DEFAULT VALUES;`
        : `INSERT INTO ${target} (${insert.values.map(cell => ident(cell.column)).join(', ')}) VALUES (${insert.values.map(cell => literal(cell.value)).join(', ')});`),
  ]
}
