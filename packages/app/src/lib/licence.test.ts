import { describe, expect, it } from 'vitest'
import { installedOutcome } from '@/lib/licence'

describe('installedOutcome', () => {
  it('says the limit is gone when the licence covers this build', () => {
    const outcome = installedOutcome({
      kind: 'licensed',
      email: 'buyer@example.com',
      name: null,
      updatesUntil: '2027-03-17T23:59:59Z',
    })

    expect(outcome.ok).toBe(true)
    expect(outcome.message).toContain('unlimited')
  })

  it('does not read as a win when the window closed before this build', () => {
    const outcome = installedOutcome({
      kind: 'expired',
      email: 'buyer@example.com',
      updatesUntil: '2025-03-17T23:59:59Z',
    })

    // The file installed and nothing unlocked. A green "licence added" here is the
    // whole reason this is two phrases and not one.
    expect(outcome.ok).toBe(false)
    expect(outcome.message).toContain('does not cover this build')
  })
})
