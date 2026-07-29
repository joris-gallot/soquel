import type { QueryColumn } from '@/lib/bindings'
import { describe, expect, it } from 'vitest'
import { buildTableChanges, emptyStaged, previewSql, stagedCount } from './staged'

const columns: QueryColumn[] = [
  { name: 'id', dataType: 'int4', kind: 'number' },
  { name: 'name', dataType: 'text', kind: 'text' },
]
const rows: (string | null)[][] = [
  ['1', 'Ada'],
  ['2', 'Alan'],
]

describe('buildTableChanges', () => {
  it('keys updates on original values even when the key column was edited', () => {
    const staged = { ...emptyStaged(), edits: { 0: { id: '10', name: 'Ada II' } } }
    const changes = buildTableChanges(staged, rows, columns, ['id'], 'app', 'customers')
    expect(changes.updates).toEqual([{
      key: [{ column: 'id', value: '1' }],
      set: [{ column: 'id', value: '10' }, { column: 'name', value: 'Ada II' }],
    }])
  })

  it('drops the edit when the row is also deleted', () => {
    const staged = { ...emptyStaged(), edits: { 1: { name: 'x' } }, deletes: [1] }
    const changes = buildTableChanges(staged, rows, columns, ['id'], 'app', 'customers')
    expect(changes.updates).toEqual([])
    expect(changes.deletes).toEqual([{ key: [{ column: 'id', value: '2' }] }])
  })

  it('keeps only provided insert columns so the rest take DEFAULT', () => {
    const staged = { ...emptyStaged(), inserts: [{ name: 'New', id: null }] }
    const changes = buildTableChanges(staged, rows, columns, ['id'], 'app', 'customers')
    expect(changes.inserts).toEqual([{
      values: [{ column: 'name', value: 'New' }, { column: 'id', value: null }],
    }])
  })
})

describe('stagedCount', () => {
  it('counts operations, not touched cells', () => {
    expect(stagedCount({
      edits: { 0: { name: 'x' }, 1: { name: 'y' } },
      deletes: [1],
      inserts: [{ name: 'z' }],
    })).toBe(3) // edit row 0, delete row 1 (its edit collapses), one insert
  })
})

describe('previewSql', () => {
  it('renders quoted display statements', () => {
    const statements = previewSql({
      schema: 'app',
      table: 'customers',
      updates: [{ key: [{ column: 'id', value: '1' }], set: [{ column: 'name', value: 'O\'Brien' }] }],
      deletes: [{ key: [{ column: 'note', value: null }] }],
      inserts: [{ values: [] }],
    })
    expect(statements).toEqual([
      `UPDATE "app"."customers" SET "name" = 'O''Brien' WHERE "id" = '1';`,
      `DELETE FROM "app"."customers" WHERE "note" IS NULL;`,
      `INSERT INTO "app"."customers" DEFAULT VALUES;`,
    ])
  })
})
