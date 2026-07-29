import { describe, expect, it } from 'vitest'
import { highlightJson } from './highlight-json'

describe('highlightJson', () => {
  it('tags keys, strings and numbers with token classes', () => {
    const html = highlightJson('{"plan": "pro", "seats": 3, "active": true}')
    expect(html).toContain('<span class="tok-propertyName">"plan"</span>')
    expect(html).toContain('<span class="tok-string">"pro"</span>')
    expect(html).toContain('<span class="tok-number">3</span>')
    expect(html).toContain('true')
  })

  it('escapes html inside values', () => {
    const html = highlightJson('{"markup": "<img src=x>"}')
    expect(html).not.toContain('<img')
    expect(html).toContain('&lt;img src=x&gt;')
  })

  it('keeps line breaks from pretty-printed input', () => {
    const html = highlightJson('{\n  "a": 1\n}')
    expect(html.split('\n')).toHaveLength(3)
  })
})
