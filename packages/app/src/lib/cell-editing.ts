import type { QueryColumn } from '@/lib/bindings'

export type EditorMode = 'bool' | 'date' | 'json' | 'text'

/// Sentinel select value: a real cell value can never collide with it.
export const NULL_OPTION = '__null__'

export function editorMode(column: QueryColumn): EditorMode {
  if (column.kind === 'bool')
    return 'bool'
  // Only plain dates: datetime-local would drop timezone and microseconds.
  if (column.dataType === 'date')
    return 'date'
  if (column.kind === 'json')
    return 'json'
  return 'text'
}

export function initialEditorValue(mode: EditorMode, initial: string | null): string {
  if (mode === 'bool' && initial === null)
    return NULL_OPTION
  return initial ?? ''
}

export function stagedValue(mode: EditorMode, value: string): string | null {
  if (mode === 'bool')
    return value === NULL_OPTION ? null : value
  // A cleared date reads as NULL: '' can never cast to date anyway.
  if (mode === 'date' && value === '')
    return null
  return value
}

/// Gate for staging/navigation: invalid JSON must never reach the staging area.
export function editorValueValid(mode: EditorMode, value: string): boolean {
  if (mode !== 'json' || value.trim() === '')
    return true
  try {
    JSON.parse(value)
    return true
  }
  catch {
    return false
  }
}

export interface CellPosition {
  rowIndex: number
  position: number
}

/// Tab-order neighbor among editable columns, wrapping across rows;
/// null when stepping past either end of the loaded rows.
export function nextEditablePosition(
  current: CellPosition,
  direction: 1 | -1,
  columnCount: number,
  rowCount: number,
): CellPosition | null {
  if (columnCount === 0)
    return null
  let position = current.position + direction
  let rowIndex = current.rowIndex
  if (position < 0) {
    position = columnCount - 1
    rowIndex -= 1
  }
  else if (position >= columnCount) {
    position = 0
    rowIndex += 1
  }
  if (rowIndex < 0 || rowIndex >= rowCount)
    return null
  return { rowIndex, position }
}
