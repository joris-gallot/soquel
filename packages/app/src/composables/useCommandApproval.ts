import type { Error as CoreError, SecretSubject } from '@/lib/bindings'
import { computed, onScopeDispose, ref } from 'vue'
import { commands } from '@/lib/bindings'
import { CommandError, unwrap } from '@/lib/result'

type ApprovalError = Extract<CoreError, { kind: 'command-approval-required' }>

export interface PendingCommand {
  subject: SecretSubject
  targetId: string
  targetName: string
  /// Resolved argv, placeholders substituted: what will actually run.
  argv: string[]
  retry: () => Promise<void>
}

const pending = ref<PendingCommand | null>(null)
const inlineHandlers = ref(0)

export function useCommandApproval() {
  function intercept(error: unknown, retry: () => Promise<void>): boolean {
    if (!(error instanceof CommandError) || error.raw.kind !== 'command-approval-required')
      return false
    const raw = error.raw as ApprovalError
    pending.value = {
      subject: raw.subject,
      targetId: raw.targetId,
      targetName: raw.targetName,
      argv: [raw.program, ...raw.args],
      retry,
    }
    return true
  }

  async function approve() {
    const current = pending.value
    if (!current)
      return
    unwrap(await commands.approveCredentialCommand(current.subject, current.targetId))
    pending.value = null
    await current.retry()
  }

  function dismiss() {
    pending.value = null
  }

  /// An open form shows the prompt inline: a second modal would be dismissed
  /// by the first dialog's focus trap.
  function claimInline() {
    inlineHandlers.value += 1
    onScopeDispose(() => {
      inlineHandlers.value -= 1
    })
  }

  return {
    pending,
    showAsDialog: computed(() => pending.value !== null && inlineHandlers.value === 0),
    intercept,
    approve,
    dismiss,
    claimInline,
  }
}
