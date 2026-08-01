import { beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope } from 'vue'
import { commands } from '@/lib/bindings'
import { CommandError } from '@/lib/result'
import { useSecretPrompt } from './useSecretPrompt'

vi.mock('@/lib/bindings', () => ({
  commands: { unlockSecret: vi.fn(async () => ({ status: 'ok', data: null })) },
}))

function secretRequired() {
  return new CommandError({
    kind: 'secret-required',
    message: 'prod asks for its password at each connection',
    subject: 'connection',
    targetId: 'conn-1',
    targetName: 'prod',
  })
}

describe('useSecretPrompt', () => {
  beforeEach(() => {
    useSecretPrompt().dismiss()
    vi.mocked(commands.unlockSecret).mockClear()
  })

  it('captures secret-required errors and ignores the rest', () => {
    const { intercept, pending } = useSecretPrompt()
    expect(intercept(new CommandError({ kind: 'database', message: 'boom' }), async () => {})).toBe(false)
    expect(pending.value).toBeNull()

    expect(intercept(secretRequired(), async () => {})).toBe(true)
    expect(pending.value).toMatchObject({ subject: 'connection', targetId: 'conn-1', targetName: 'prod' })
  })

  it('yields the dialog to an inline panel while one is mounted', () => {
    const { intercept, showAsDialog, claimInline } = useSecretPrompt()
    intercept(secretRequired(), async () => {})
    expect(showAsDialog.value).toBe(true)

    const scope = effectScope()
    scope.run(() => claimInline())
    expect(showAsDialog.value).toBe(false)

    scope.stop()
    expect(showAsDialog.value).toBe(true)
  })

  it('hands the password to the core, then retries', async () => {
    const { intercept, unlock, pending } = useSecretPrompt()
    const retry = vi.fn(async () => {})
    intercept(secretRequired(), retry)

    await unlock('s3cret', true)
    expect(commands.unlockSecret).toHaveBeenCalledWith('connection', 'conn-1', 's3cret', true)
    expect(retry).toHaveBeenCalledOnce()
    expect(pending.value).toBeNull()
  })
})
