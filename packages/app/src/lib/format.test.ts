import { describe, expect, it } from 'vitest'
import { formatDay, formatEstimatedRows } from './format'

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

describe('formatDay', () => {
  it('names the month, so the day is the same on both sides of the Atlantic', () => {
    expect(formatDay('2026-12-01T00:00:00Z')).toBe('December 1, 2026')
  })

  it('reads the day in UTC, the way the licence window was signed', () => {
    // Local time would call this December 31 anywhere behind UTC, a day short
    // of what the customer paid for.
    expect(formatDay('2026-01-01T00:00:00Z')).toBe('January 1, 2026')
  })

  it('has nothing to show for a missing or unparseable date', () => {
    expect(formatDay(null)).toBeNull()
    expect(formatDay(undefined)).toBeNull()
    expect(formatDay('whenever')).toBeNull()
  })
})
