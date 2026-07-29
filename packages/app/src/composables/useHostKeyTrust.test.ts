import { beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope } from 'vue'
import { CommandError } from '@/lib/result'
import { useHostKeyTrust } from './useHostKeyTrust'

vi.mock('@/lib/bindings', () => ({
  commands: { trustHostKey: vi.fn(async () => ({ status: 'ok', data: null })) },
}))

function hostKeyError(previouslyTrusted = false) {
  return new CommandError({
    kind: 'host-key-untrusted',
    message: 'host key for bastion:2222 is not trusted',
    host: 'bastion',
    port: 2222,
    fingerprint: 'SHA256:abc',
    key: 'ssh-ed25519 AAAA',
    previouslyTrusted,
  })
}

describe('useHostKeyTrust', () => {
  beforeEach(() => {
    useHostKeyTrust().dismiss()
  })

  it('captures host-key errors and ignores the rest', () => {
    const { intercept, pending } = useHostKeyTrust()
    expect(intercept(new CommandError({ kind: 'database', message: 'boom' }), async () => {})).toBe(false)
    expect(pending.value).toBeNull()

    expect(intercept(hostKeyError(), async () => {})).toBe(true)
    expect(pending.value).toMatchObject({ host: 'bastion', port: 2222, fingerprint: 'SHA256:abc' })
  })

  it('yields the dialog to an inline panel while one is mounted', () => {
    const { intercept, showAsDialog, claimInline } = useHostKeyTrust()
    intercept(hostKeyError(), async () => {})
    expect(showAsDialog.value).toBe(true)

    // A mounted form panel claims the prompt: a second modal would be dismissed
    // by the open dialog's focus trap.
    const scope = effectScope()
    scope.run(() => claimInline())
    expect(showAsDialog.value).toBe(false)

    scope.stop()
    expect(showAsDialog.value).toBe(true)
  })

  it('retries the failed action after trusting', async () => {
    const { intercept, trust, pending } = useHostKeyTrust()
    const retry = vi.fn(async () => {})
    intercept(hostKeyError(), retry)

    await trust()
    expect(retry).toHaveBeenCalledOnce()
    expect(pending.value).toBeNull()
  })
})
