import type { QueryColumn } from '@/lib/bindings'
import { describe, expect, it } from 'vitest'
import {
  editorMode,
  editorValueValid,
  initialEditorValue,
  nextEditablePosition,
  NULL_OPTION,
  stagedValue,
} from '@/lib/cell-editing'

function column(kind: QueryColumn['kind'], dataType: string | null = null): QueryColumn {
  return { name: 'c', dataType, kind }
}

describe('editorMode', () => {
  it('picks the editor from kind and exact data type', () => {
    expect(editorMode(column('bool', 'bool'))).toBe('bool')
    expect(editorMode(column('date-time', 'date'))).toBe('date')
    // Timestamps stay text: datetime-local would mangle tz and precision.
    expect(editorMode(column('date-time', 'timestamptz'))).toBe('text')
    expect(editorMode(column('json', 'jsonb'))).toBe('json')
    expect(editorMode(column('number', 'int4'))).toBe('text')
    expect(editorMode(column('text'))).toBe('text')
  })
})

describe('initialEditorValue / stagedValue roundtrip', () => {
  it('maps NULL through the bool sentinel', () => {
    expect(initialEditorValue('bool', null)).toBe(NULL_OPTION)
    expect(stagedValue('bool', NULL_OPTION)).toBeNull()
    expect(initialEditorValue('bool', 'true')).toBe('true')
    expect(stagedValue('bool', 'false')).toBe('false')
  })

  it('reads a cleared date as NULL but keeps empty text as empty string', () => {
    expect(stagedValue('date', '')).toBeNull()
    expect(stagedValue('date', '2026-07-30')).toBe('2026-07-30')
    expect(stagedValue('text', '')).toBe('')
    expect(initialEditorValue('text', null)).toBe('')
  })
})

describe('editorValueValid', () => {
  it('gates only json, and lets empty pass', () => {
    expect(editorValueValid('json', '{"a": 1}')).toBe(true)
    expect(editorValueValid('json', '{a: 1}')).toBe(false)
    expect(editorValueValid('json', '  ')).toBe(true)
    expect(editorValueValid('text', '{a: 1}')).toBe(true)
  })
})

describe('nextEditablePosition', () => {
  const step = (rowIndex: number, position: number, direction: 1 | -1) =>
    nextEditablePosition({ rowIndex, position }, direction, 3, 2)

  it('moves within a row and wraps across rows', () => {
    expect(step(0, 0, 1)).toEqual({ rowIndex: 0, position: 1 })
    expect(step(0, 2, 1)).toEqual({ rowIndex: 1, position: 0 })
    expect(step(1, 0, -1)).toEqual({ rowIndex: 0, position: 2 })
  })

  it('stops past either end and on empty columns', () => {
    expect(step(0, 0, -1)).toBeNull()
    expect(step(1, 2, 1)).toBeNull()
    expect(nextEditablePosition({ rowIndex: 0, position: 0 }, 1, 0, 2)).toBeNull()
  })
})
