import { describe, expect, it } from 'vitest'
import { formatTtl, KEY_KIND_BADGE } from './kv'

describe('formatTtl', () => {
  it('scales the unit with the duration', () => {
    expect(formatTtl(500)).toBe('1s')
    expect(formatTtl(42_000)).toBe('42s')
    expect(formatTtl(300_000)).toBe('5m')
    expect(formatTtl(7_200_000)).toBe('2h')
    expect(formatTtl(172_800_000)).toBe('2d')
  })
})

describe('kEY_KIND_BADGE', () => {
  it('gives every kind a distinct short label', () => {
    const shorts = Object.values(KEY_KIND_BADGE).map(badge => badge.short)
    expect(new Set(shorts).size).toBe(shorts.length)
  })
})
