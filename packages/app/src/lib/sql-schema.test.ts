import type { SchemaSnapshot, TableInfo } from '@/lib/bindings'
import { describe, expect, it } from 'vitest'
import { snapshotToNamespace } from './sql-schema'

function table(name: string, columns: string[]): TableInfo {
  return {
    name,
    kind: 'table',
    estimatedRows: 0,
    columns: columns.map(column => ({ name: column, dataType: 'text', nullable: true, default: null })),
    primaryKey: [],
    indexes: [],
    foreignKeys: [],
  }
}

describe('snapshotToNamespace', () => {
  it('maps schemas to tables to column lists', () => {
    const snapshot: SchemaSnapshot = {
      schemas: [
        { name: 'app', tables: [table('customers', ['id', 'name']), table('orders', ['id', 'total'])] },
        { name: 'public', tables: [table('migrations', ['version'])] },
      ],
    }
    expect(snapshotToNamespace(snapshot)).toEqual({
      app: { customers: ['id', 'name'], orders: ['id', 'total'] },
      public: { migrations: ['version'] },
    })
  })

  it('handles an empty snapshot', () => {
    expect(snapshotToNamespace({ schemas: [] })).toEqual({})
  })
})
