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
  /// Whole label rather than a modifier the caller concatenates: the non-mac one
  /// needs a space, and a constant whose trailing space matters gets eaten.
  const paletteShortcut = computed(() => (isMac.value ? '⌘K' : 'Ctrl K'))

  return { os, isMac, paletteShortcut, load }
}
