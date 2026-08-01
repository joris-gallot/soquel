import type { Error as CoreError } from '@/lib/bindings'
import { computed, onScopeDispose, ref } from 'vue'
import { commands } from '@/lib/bindings'
import { CommandError, unwrap } from '@/lib/result'

type SecretRequiredError = Extract<CoreError, { kind: 'secret-required' }>

export interface PendingSecret {
  connectionId: string
  connectionName: string
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
      connectionId: raw.connectionId,
      connectionName: raw.connectionName,
      retry,
    }
    return true
  }

  async function unlock(secret: string, remember: boolean) {
    const current = pending.value
    if (!current)
      return
    unwrap(await commands.unlockConnection(current.connectionId, secret, remember))
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
