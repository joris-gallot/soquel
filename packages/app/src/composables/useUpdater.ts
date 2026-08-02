import type { UpdateInfo } from '@/lib/bindings'
import { computed, ref } from 'vue'
import { commands, events } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

const available = ref<UpdateInfo | null>(null)
const panelOpen = ref(false)
const downloading = ref(false)
const downloaded = ref(0)
const total = ref<number | null>(null)
let listening = false

export function useUpdater() {
  // One listener for the app: the panel mounts once but composables re-run.
  async function listen() {
    if (listening)
      return
    listening = true
    await events.updateProgress.listen(({ payload }) => {
      downloaded.value = payload.downloaded ?? 0
      total.value = payload.total
    })
  }

  /// An unreachable endpoint is the normal offline case, never worth a toast.
  async function check(): Promise<boolean> {
    try {
      available.value = unwrap(await commands.checkUpdate())
    }
    catch {
      available.value = null
    }
    return available.value !== null
  }

  /// Resolves only on failure: a successful install restarts the app.
  async function install() {
    downloading.value = true
    downloaded.value = 0
    total.value = null
    try {
      unwrap(await commands.installUpdate())
    }
    finally {
      downloading.value = false
    }
  }

  const progress = computed(() => {
    if (!downloading.value || total.value === null)
      return null
    return Math.min(1, downloaded.value / total.value)
  })

  /// The installer runs after the last chunk, while the app is still up.
  const installing = computed(() =>
    downloading.value && total.value !== null && downloaded.value >= total.value,
  )

  return {
    available,
    panelOpen,
    downloading,
    downloaded,
    total,
    progress,
    installing,
    check,
    install,
    listen,
  }
}
