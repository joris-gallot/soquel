import type { McpApprovalRequest } from '@/lib/bindings'
import { computed, ref } from 'vue'
import { commands, events } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

const queue = ref<McpApprovalRequest[]>([])
let listening = false

export function useAgentApprovals() {
  // One listener for the app: the dialog mounts once but composables re-run.
  async function listen() {
    if (listening)
      return
    listening = true
    await events.mcpApprovalRequest.listen(({ payload }) => {
      queue.value = [...queue.value, payload]
    })
  }

  const pending = computed(() => queue.value[0] ?? null)

  async function resolve(approved: boolean) {
    const request = pending.value
    if (!request)
      return
    queue.value = queue.value.slice(1)
    // A request the core already timed out is gone: nothing left to answer.
    try {
      unwrap(await commands.mcpResolveApproval(request.id, approved))
    }
    catch {}
  }

  return { pending, queue, listen, resolve }
}
