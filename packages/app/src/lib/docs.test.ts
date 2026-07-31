import { describe, expect, it } from 'vitest'
import { compactCount, DOC_KIND_BADGE, docIdLabel, docPreview, formatBytes, formatDocCount } from './docs'

describe('docIdLabel', () => {
  it('unwraps canonical wrapper types', () => {
    expect(docIdLabel('{"$oid":"66a1b2c3d4e5f6a7b8c9d0e1"}')).toBe('66a1b2c3d4e5f6a7b8c9d0e1')
    expect(docIdLabel('{"$numberLong":"9007199254740993"}')).toBe('9007199254740993')
    expect(docIdLabel('{"$numberInt":"7"}')).toBe('7')
  })

  it('keeps bare strings and numbers as-is', () => {
    expect(docIdLabel('"user-42"')).toBe('user-42')
    expect(docIdLabel('42')).toBe('42')
  })

  it('renders canonical dates as ISO', () => {
    expect(docIdLabel('{"$date":{"$numberLong":"1722000000000"}}')).toBe('2024-07-26T13:20:00.000Z')
  })

  it('compacts compound ids', () => {
    expect(docIdLabel('{"tenant":"acme","seq":{"$numberInt":"1"}}')).toBe('{tenant: acme, seq: 1}')
  })

  it('handles the missing and the malformed', () => {
    expect(docIdLabel(null)).toBe('no _id')
    expect(docIdLabel('not json')).toBe('not json')
  })
})

describe('docPreview', () => {
  it('drops _id and joins fields on one line', () => {
    const doc = '{"_id":{"$oid":"66a1b2c3d4e5f6a7b8c9d0e1"},"name":"Ada","plan":"pro"}'
    expect(docPreview(doc)).toBe('name: Ada  plan: pro')
  })

  it('compacts nested documents and arrays', () => {
    expect(docPreview('{"profile":{"city":"Lyon"},"tags":["a","b"]}'))
      .toBe('profile: {city: Lyon}  tags: [a, b]')
  })

  it('truncates long previews', () => {
    const doc = JSON.stringify({ text: 'x'.repeat(300) })
    expect(docPreview(doc).length).toBe(140)
    expect(docPreview(doc).endsWith('…')).toBe(true)
  })

  it('labels empty documents instead of showing nothing', () => {
    expect(docPreview('{"_id":"a"}')).toBe('(empty document)')
  })
})

describe('formatDocCount', () => {
  it('prefixes estimates with ~ and compacts them', () => {
    expect(formatDocCount(52_400, false)).toBe('~52.4k docs')
  })

  it('keeps exact counts exact', () => {
    expect(formatDocCount(1245, true)).toBe('1245 docs')
    expect(formatDocCount(1, true)).toBe('1 doc')
  })
})

describe('compactCount', () => {
  it('scales the unit with the magnitude', () => {
    expect(compactCount(950)).toBe('950')
    expect(compactCount(1200)).toBe('1.2k')
    expect(compactCount(45_000_000)).toBe('45m')
  })
})

describe('formatBytes', () => {
  it('scales the unit with the size', () => {
    expect(formatBytes(512)).toBe('512 B')
    expect(formatBytes(2048)).toBe('2.0 KB')
    expect(formatBytes(13_631_488)).toBe('13 MB')
  })
})

describe('dOC_KIND_BADGE', () => {
  it('gives every kind a distinct short label', () => {
    const shorts = Object.values(DOC_KIND_BADGE).map(badge => badge.short)
    expect(new Set(shorts).size).toBe(shorts.length)
  })
})
