import { describe, expect, it, vi } from 'vitest'
import { useKeychain } from './useKeychain'

const status = { keychain: false, problem: 'No keyring answered on the D-Bus session.' }
const secretsStatus = vi.fn(async () => ({ status: 'ok' as const, data: status }))

vi.mock('@/lib/bindings', () => ({
  commands: { secretsStatus: () => secretsStatus() },
}))

describe('useKeychain', () => {
  it('fetches once for the whole app run', async () => {
    const { load, available, problem } = useKeychain()

    // The core probes at startup; the answer cannot change without a restart.
    await load()
    await load()

    expect(secretsStatus).toHaveBeenCalledTimes(1)
    expect(available.value).toBe(false)
    expect(problem.value).toBe(status.problem)
  })
})
