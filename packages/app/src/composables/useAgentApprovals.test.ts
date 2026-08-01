import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useAgentApprovals } from './useAgentApprovals'

type Reply = { status: 'ok', data: null } | { status: 'error', error: { kind: string, message: string } }

const resolveApproval = vi.fn(async (): Promise<Reply> => ({ status: 'ok', data: null }))

vi.mock('@/lib/bindings', () => ({
  commands: { mcpResolveApproval: (...args: unknown[]) => resolveApproval(...(args as [])) },
  events: { mcpApprovalRequest: { listen: vi.fn(async () => () => {}) } },
}))

const toastError = vi.fn()
vi.mock('vue-sonner', () => ({ toast: { error: (message: string) => toastError(message) } }))

function request(id: string, operation: string) {
  return { id, connectionId: 'c1', connectionName: 'agent sqlite', operation, payload: null }
}

describe('useAgentApprovals', () => {
  beforeEach(async () => {
    const { queue } = useAgentApprovals()
    queue.value = []
    resolveApproval.mockClear()
    resolveApproval.mockResolvedValue({ status: 'ok', data: null })
    toastError.mockClear()
  })

  it('answers each queued request with its own id', async () => {
    const { queue, pending, resolve } = useAgentApprovals()
    queue.value = [request('a', 'DELETE FROM one'), request('b', 'DELETE FROM two')]

    expect(pending.value?.id).toBe('a')
    await resolve(true)
    expect(resolveApproval).toHaveBeenLastCalledWith('a', true)

    // The second request stays pending, untouched by the first answer.
    expect(pending.value?.id).toBe('b')
    await resolve(false)
    expect(resolveApproval).toHaveBeenLastCalledWith('b', false)
    expect(pending.value).toBeNull()
  })

  it('says so when the core already dropped the request', async () => {
    const { queue, resolve } = useAgentApprovals()
    queue.value = [request('stale', 'DELETE FROM one')]
    resolveApproval.mockResolvedValue({
      status: 'error',
      error: { kind: 'not-found', message: 'approval request stale is no longer pending' },
    })

    await resolve(true)
    // Silence here would read as "your write ran".
    expect(toastError).toHaveBeenCalledWith('That request expired. The agent has to ask again.')
  })
})
