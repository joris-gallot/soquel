import { computed, ref } from 'vue'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

/// The core reports its compile target once; it cannot change while running.
const os = ref('')
let loaded = false

export function useOs() {
  async function load() {
    if (loaded)
      return
    loaded = true
    os.value = unwrap(await commands.platform())
  }

  const isMac = computed(() => os.value === 'macos')
  /// Ctrl on Windows and Linux, where the palette binds both anyway.
  const modifier = computed(() => (isMac.value ? '⌘' : 'Ctrl '))

  return { os, isMac, modifier, load }
}
