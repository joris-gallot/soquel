import { describe, expect, it } from 'vitest'
import { FILTER_OPS_BY_KIND, filterLabel, OP_NEEDS_VALUE } from './filters'

describe('fILTER_OPS_BY_KIND', () => {
  it('gives numbers comparisons but no text search', () => {
    expect(FILTER_OPS_BY_KIND.number).toContain('gt')
    expect(FILTER_OPS_BY_KIND.number).not.toContain('contains')
  })

  it('limits bytes to nullness checks', () => {
    expect(FILTER_OPS_BY_KIND.bytes).toEqual(['is-null', 'is-not-null'])
  })

  it('every kind can check for null', () => {
    for (const ops of Object.values(FILTER_OPS_BY_KIND))
      expect(ops).toContain('is-null')
  })
})

describe('filterLabel', () => {
  it('includes the value only when the operator takes one', () => {
    expect(filterLabel({ column: 'name', op: 'contains', value: 'ada' })).toBe('name contains ada')
    expect(filterLabel({ column: 'email', op: 'is-null', value: null })).toBe('email is null')
  })

  it('agrees with OP_NEEDS_VALUE', () => {
    expect(OP_NEEDS_VALUE['is-not-null']).toBe(false)
    expect(OP_NEEDS_VALUE.contains).toBe(true)
  })
})
