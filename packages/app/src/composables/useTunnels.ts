import type { TunnelInput, TunnelProfile } from '@/lib/bindings'
import { ref } from 'vue'
import { useHostKeyTrust } from '@/composables/useHostKeyTrust'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

const tunnels = ref<TunnelProfile[]>([])

export function useTunnels() {
  async function refresh() {
    tunnels.value = unwrap(await commands.listTunnels())
  }

  async function create(input: TunnelInput) {
    const tunnel = unwrap(await commands.createTunnel(input))
    await refresh()
    return tunnel
  }

  async function update(id: string, input: TunnelInput) {
    const tunnel = unwrap(await commands.updateTunnel(id, input))
    await refresh()
    return tunnel
  }

  async function remove(id: string) {
    unwrap(await commands.deleteTunnel(id))
    await refresh()
  }

  const { intercept } = useHostKeyTrust()

  async function test(input: TunnelInput, existingId?: string) {
    try {
      unwrap(await commands.testTunnel(input, existingId ?? null))
    }
    catch (error) {
      intercept(error, () => test(input, existingId))
      throw error
    }
  }

  return { tunnels, refresh, create, update, remove, test }
}
