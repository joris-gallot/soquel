import type { McpStatus } from '@/lib/bindings'
import { ref } from 'vue'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

const status = ref<McpStatus | null>(null)

export function useMcp() {
  async function refresh() {
    status.value = unwrap(await commands.mcpStatus())
  }

  async function start() {
    status.value = unwrap(await commands.mcpStart(null))
  }

  async function stop() {
    unwrap(await commands.mcpStop())
    await refresh()
  }

  async function regenerateToken() {
    status.value = unwrap(await commands.mcpRegenerateToken())
  }

  return { status, refresh, start, stop, regenerateToken }
}
