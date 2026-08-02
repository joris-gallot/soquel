import type { LicenceStatus } from '@/lib/bindings'
import { computed, ref } from 'vue'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'
import { FREE_TABS } from '@/lib/tabs'

/// Read once at startup: the core reads a file, and nothing changes it but the
/// dialog below, which refreshes it itself.
const status = ref<LicenceStatus>({ kind: 'free' })
const panelOpen = ref(false)
let loaded = false

export function useLicence() {
  async function load() {
    if (loaded)
      return
    loaded = true
    try {
      status.value = unwrap(await commands.licenceStatus())
    }
    catch {
      // A licence that will not load is the free tier, not a startup failure.
      status.value = { kind: 'free' }
    }
  }

  /// Throws so the dialog can say why: this one is a deliberate action whose
  /// failure the user has to see, unlike the read above.
  async function install(token: string) {
    status.value = unwrap(await commands.installLicence(token))
  }

  const unlocked = computed(() => status.value.kind === 'licensed')
  const tabLimit = computed(() => (unlocked.value ? Number.POSITIVE_INFINITY : FREE_TABS))

  return { status, unlocked, tabLimit, panelOpen, load, install }
}
