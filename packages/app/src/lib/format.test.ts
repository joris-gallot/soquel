import { describe, expect, it } from 'vitest'
import { formatEstimatedRows } from './format'

describe('formatEstimatedRows', () => {
  it('hides the never-analyzed sentinel and null', () => {
    expect(formatEstimatedRows(-1)).toBe('')
    expect(formatEstimatedRows(null)).toBe('')
  })

  it('formats small and compact large counts', () => {
    expect(formatEstimatedRows(0)).toBe('0')
    expect(formatEstimatedRows(42)).toBe('42')
    expect(formatEstimatedRows(1234)).toBe('1.2K')
    expect(formatEstimatedRows(5_600_000)).toBe('5.6M')
  })
})
