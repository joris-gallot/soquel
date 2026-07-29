import type { ConnectionInput, ConnectionProfile } from '@/lib/bindings'
import { ref } from 'vue'
import { useHostKeyTrust } from '@/composables/useHostKeyTrust'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

const connections = ref<ConnectionProfile[]>([])
const activeIds = ref<Set<string>>(new Set())

export function useConnections() {
  async function refresh() {
    connections.value = unwrap(await commands.listConnections())
    activeIds.value = new Set(unwrap(await commands.activeConnections()))
  }

  async function create(input: ConnectionInput) {
    const profile = unwrap(await commands.createConnection(input))
    await refresh()
    return profile
  }

  async function update(id: string, input: ConnectionInput) {
    const profile = unwrap(await commands.updateConnection(id, input))
    await refresh()
    return profile
  }

  async function remove(id: string) {
    unwrap(await commands.deleteConnection(id))
    await refresh()
  }

  const { intercept } = useHostKeyTrust()

  async function connect(id: string) {
    try {
      unwrap(await commands.connect(id))
    }
    catch (error) {
      intercept(error, () => connect(id))
      throw error
    }
    await refresh()
  }

  async function disconnect(id: string) {
    unwrap(await commands.disconnect(id))
    await refresh()
  }

  async function test(input: ConnectionInput, existingId?: string) {
    try {
      unwrap(await commands.testConnection(input, existingId ?? null))
    }
    catch (error) {
      intercept(error, () => test(input, existingId))
      throw error
    }
  }

  return { connections, activeIds, refresh, create, update, remove, connect, disconnect, test }
}
