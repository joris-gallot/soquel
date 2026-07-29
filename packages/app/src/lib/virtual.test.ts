import { describe, expect, it } from 'vitest'
import { windowFor } from './virtual'

describe('windowFor', () => {
  it('renders everything when the rows fit the viewport', () => {
    const window = windowFor(0, 600, 25, 3)
    expect(window).toEqual({ start: 0, end: 3, padTop: 0, padBottom: 0 })
  })

  it('slices around the scroll position with overscan', () => {
    const window = windowFor(2500, 500, 25, 10_000, 10)
    expect(window.start).toBe(90) // 2500/25 - 10 overscan
    expect(window.end).toBe(90 + 20 + 20)
    expect(window.padTop).toBe(90 * 25)
    expect(window.padBottom).toBe((10_000 - window.end) * 25)
  })

  it('clamps at the end of the list', () => {
    const window = windowFor(999_999_999, 500, 25, 1000)
    expect(window.end).toBe(1000)
    expect(window.padBottom).toBe(0)
    expect(window.start).toBeLessThanOrEqual(1000)
  })
})
