import type { TrustWindowInfo } from '@/lib/bindings'
import { useIntervalFn } from '@vueuse/core'
import { computed, ref } from 'vue'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

const granted = ref<TrustWindowInfo[]>([])
const now = ref(Date.now())

export async function refreshTrustWindows() {
  granted.value = unwrap(await commands.mcpTrustWindows())
}

export function useTrustWindows() {
  // A window ends on its own: tick locally so the row leaves without a round trip.
  useIntervalFn(() => {
    now.value = Date.now()
  }, 1000)

  // specta renders f64 as nullable; a window without an end has already ended.
  const windows = computed(() => granted.value
    .filter(entry => (entry.expiresAtMs ?? 0) > now.value)
    .map(entry => ({ ...entry, remaining: remainingLabel((entry.expiresAtMs ?? 0) - now.value) })))

  async function revoke(entry: TrustWindowInfo) {
    unwrap(await commands.mcpRevokeTrust(entry.session, entry.connectionId))
    await refreshTrustWindows()
  }

  return { windows, refresh: refreshTrustWindows, revoke }
}

export function remainingLabel(ms: number) {
  const total = Math.max(0, Math.ceil(ms / 1000))
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`
}
