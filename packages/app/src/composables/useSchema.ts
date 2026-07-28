import type { SchemaSnapshot } from '@/lib/bindings'
import { ref } from 'vue'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

const snapshots = ref<Record<string, SchemaSnapshot>>({})
const pending = ref<Record<string, boolean>>({})

export function useSchema() {
  async function load(id: string, force = false) {
    if (!force && snapshots.value[id])
      return
    pending.value[id] = true
    try {
      snapshots.value[id] = unwrap(await commands.schemaSnapshot(id))
    }
    finally {
      pending.value[id] = false
    }
  }

  function evict(id: string) {
    delete snapshots.value[id]
  }

  return { snapshots, pending, load, evict }
}
