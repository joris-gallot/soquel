import type { Error as CoreError } from '@/lib/bindings'
import { computed, onScopeDispose, ref } from 'vue'
import { commands } from '@/lib/bindings'
import { CommandError, unwrap } from '@/lib/result'

type HostKeyError = Extract<CoreError, { kind: 'host-key-untrusted' }>

export interface PendingHostKey {
  host: string
  port: number
  fingerprint: string
  key: string
  previouslyTrusted: boolean
  retry: () => Promise<void>
}

const pending = ref<PendingHostKey | null>(null)
const inlineHandlers = ref(0)

export function useHostKeyTrust() {
  /// Captures host-key errors; the trust prompt takes over and can retry.
  function intercept(error: unknown, retry: () => Promise<void>): boolean {
    if (!(error instanceof CommandError) || error.raw.kind !== 'host-key-untrusted')
      return false
    const raw = error.raw as HostKeyError
    pending.value = {
      host: raw.host,
      port: raw.port,
      fingerprint: raw.fingerprint,
      key: raw.key,
      previouslyTrusted: raw.previouslyTrusted,
      retry,
    }
    return true
  }

  async function trust() {
    const current = pending.value
    if (!current)
      return
    unwrap(await commands.trustHostKey(current.host, current.port, current.key))
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
    trust,
    dismiss,
    claimInline,
  }
}
