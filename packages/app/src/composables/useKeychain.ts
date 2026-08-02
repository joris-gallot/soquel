import { ref } from 'vue'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

/// Probed once by the core at startup, so one fetch per app run is enough.
const available = ref(true)
const problem = ref<string | null>(null)
let loaded = false

export function useKeychain() {
  async function load() {
    if (loaded)
      return
    loaded = true
    const status = unwrap(await commands.secretsStatus())
    available.value = status.keychain
    problem.value = status.problem
  }

  return { available, problem, load }
}
