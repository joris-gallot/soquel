import type { Error as CoreError, SecretSubject } from '@/lib/bindings'
import { computed, onScopeDispose, ref } from 'vue'
import { commands } from '@/lib/bindings'
import { CommandError, unwrap } from '@/lib/result'

type SecretRequiredError = Extract<CoreError, { kind: 'secret-required' }>

export interface PendingSecret {
  subject: SecretSubject
  targetId: string
  targetName: string
  retry: () => Promise<void>
}

const pending = ref<PendingSecret | null>(null)
const inlineHandlers = ref(0)

export function useSecretPrompt() {
  /// Captures "this connection asks for its password"; the prompt takes over
  /// and can retry what failed.
  function intercept(error: unknown, retry: () => Promise<void>): boolean {
    if (!(error instanceof CommandError) || error.raw.kind !== 'secret-required')
      return false
    const raw = error.raw as SecretRequiredError
    pending.value = {
      subject: raw.subject,
      targetId: raw.targetId,
      targetName: raw.targetName,
      retry,
    }
    return true
  }

  async function unlock(secret: string, remember: boolean) {
    const current = pending.value
    if (!current)
      return
    unwrap(await commands.unlockSecret(current.subject, current.targetId, secret, remember))
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
    unlock,
    dismiss,
    claimInline,
  }
}
