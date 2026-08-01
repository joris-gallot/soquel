import { beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope } from 'vue'
import { commands } from '@/lib/bindings'
import { CommandError } from '@/lib/result'
import { useCommandApproval } from './useCommandApproval'

vi.mock('@/lib/bindings', () => ({
  commands: { approveCredentialCommand: vi.fn(async () => ({ status: 'ok', data: null })) },
}))

function approvalRequired() {
  return new CommandError({
    kind: 'command-approval-required',
    message: 'iam gets its password from a command that has not been approved on this machine',
    subject: 'connection',
    targetId: 'conn-1',
    targetName: 'iam',
    program: 'aws',
    args: ['rds', 'generate-db-auth-token', '--hostname', 'db.internal'],
  })
}

describe('useCommandApproval', () => {
  beforeEach(() => {
    useCommandApproval().dismiss()
    vi.mocked(commands.approveCredentialCommand).mockClear()
  })

  it('captures approval requests and ignores the rest', () => {
    const { intercept, pending } = useCommandApproval()
    expect(intercept(new CommandError({ kind: 'database', message: 'boom' }), async () => {})).toBe(false)
    expect(pending.value).toBeNull()

    expect(intercept(approvalRequired(), async () => {})).toBe(true)
    // The dialog shows the resolved argv, program first.
    expect(pending.value?.argv).toEqual(['aws', 'rds', 'generate-db-auth-token', '--hostname', 'db.internal'])
  })

  it('yields the dialog to an inline panel while one is mounted', () => {
    const { intercept, showAsDialog, claimInline } = useCommandApproval()
    intercept(approvalRequired(), async () => {})
    expect(showAsDialog.value).toBe(true)

    const scope = effectScope()
    scope.run(() => claimInline())
    expect(showAsDialog.value).toBe(false)

    scope.stop()
    expect(showAsDialog.value).toBe(true)
  })

  it('approves against the target, then retries', async () => {
    const { intercept, approve, pending } = useCommandApproval()
    const retry = vi.fn(async () => {})
    intercept(approvalRequired(), retry)

    await approve()
    expect(commands.approveCredentialCommand).toHaveBeenCalledWith('connection', 'conn-1')
    expect(retry).toHaveBeenCalledOnce()
    expect(pending.value).toBeNull()
  })

  it('dismissing approves nothing', async () => {
    const { intercept, dismiss, approve } = useCommandApproval()
    intercept(approvalRequired(), async () => {})
    dismiss()

    await approve()
    expect(commands.approveCredentialCommand).not.toHaveBeenCalled()
  })
})
