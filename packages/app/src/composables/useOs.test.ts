import { describe, expect, it, vi } from 'vitest'
import { useOs } from './useOs'

// Rust's vocabulary, not Node's: `std::env::consts::OS` says "macos" where
// process.platform would say "darwin" and navigator.platform "MacIntel".
const platform = vi.fn(async () => ({ status: 'ok' as const, data: 'macos' }))

vi.mock('@/lib/bindings', () => ({
  commands: { platform: () => platform() },
}))

describe('useOs', () => {
  it('prints the modifier of the platform the core reports', async () => {
    const { load, modifier, isMac } = useOs()

    // Ctrl until the core answers: the palette binds both, so the label is the
    // only thing at stake and the majority platform is the safer default.
    expect(modifier.value).toBe('Ctrl ')

    await load()
    await load()

    expect(isMac.value).toBe(true)
    expect(modifier.value).toBe('⌘')
    expect(platform).toHaveBeenCalledTimes(1)
  })
})
