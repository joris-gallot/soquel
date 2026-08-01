import type { ApprovalAnswer, McpApprovalRequest } from '@/lib/bindings'
import { computed, ref } from 'vue'
import { toast } from 'vue-sonner'
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

  async function resolve(answer: ApprovalAnswer) {
    const request = pending.value
    if (!request)
      return
    queue.value = queue.value.filter(entry => entry.id !== request.id)
    try {
      unwrap(await commands.mcpResolveApproval(request.id, answer))
    }
    catch {
      // The core timed the request out while the dialog was still up: say so,
      // or an approved write looks like it ran.
      toast.error('That request expired. The agent has to ask again.')
    }
  }

  return { pending, queue, listen, resolve }
}
