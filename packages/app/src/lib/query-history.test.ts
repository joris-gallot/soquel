import type { HistoryEntry } from './query-history'
import { describe, expect, it } from 'vitest'
import { filterHistory, pushHistory } from './query-history'

function entry(sql: string, at = 0): HistoryEntry {
  return { sql, at, durationMs: 1, ok: true }
}

describe('pushHistory', () => {
  it('prepends newest first', () => {
    const history = pushHistory([entry('select 1')], entry('select 2'))
    expect(history.map(e => e.sql)).toEqual(['select 2', 'select 1'])
  })

  it('collapses a rerun of the latest entry', () => {
    const history = pushHistory([entry('select 1', 1), entry('select 0')], entry('select 1', 2))
    expect(history.map(e => e.sql)).toEqual(['select 1', 'select 0'])
    expect(history[0].at).toBe(2)
  })

  it('caps at 100 entries', () => {
    const full = Array.from({ length: 100 }, (_, i) => entry(`select ${i}`))
    const history = pushHistory(full, entry('select new'))
    expect(history).toHaveLength(100)
    expect(history[0].sql).toBe('select new')
  })
})

describe('filterHistory', () => {
  const entries = [entry('SELECT * FROM customers'), entry('update orders set total = 0')]

  it('matches case-insensitively', () => {
    expect(filterHistory(entries, 'customers')).toHaveLength(1)
    expect(filterHistory(entries, 'UPDATE')).toHaveLength(1)
  })

  it('returns everything for a blank query', () => {
    expect(filterHistory(entries, '  ')).toHaveLength(2)
  })
})
