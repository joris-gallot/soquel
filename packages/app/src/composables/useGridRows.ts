import type { QueryColumn, RowsChunk, TableRowsRequest } from '@/lib/bindings'
import { Channel } from '@tauri-apps/api/core'
import { ref, shallowRef, triggerRef } from 'vue'
import { toast } from 'vue-sonner'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

export const FETCH_SIZE = 2000

/// Streamed, windowed table rows: `fetchRows(true)` restarts from the top,
/// `fetchRows(false)` appends the next window (infinite scroll). Stale
/// generations are ignored so a late chunk never corrupts a newer fetch.
export function useGridRows(
  connectionId: () => string,
  request: () => Omit<TableRowsRequest, 'limit' | 'offset'>,
) {
  const columns = ref<QueryColumn[]>([])
  // shallowRef + explicit triggers: rows can reach 6 digits, deep proxying costs.
  const rows = shallowRef<(string | null)[][]>([])
  const durationMs = ref(0)
  const loading = ref(false)
  const fetchedAll = ref(false)
  let generation = 0

  async function fetchRows(reset = true): Promise<void> {
    const mine = ++generation
    loading.value = true
    if (reset) {
      rows.value = []
      triggerRef(rows)
    }
    const offset = reset ? 0 : rows.value.length
    const channel = new Channel<RowsChunk>()
    channel.onmessage = (chunk) => {
      if (mine !== generation)
        return
      if (chunk.columns)
        columns.value = chunk.columns
      rows.value.push(...chunk.rows)
      triggerRef(rows)
    }
    try {
      const summary = unwrap(await commands.streamTableRows(
        connectionId(),
        { ...request(), limit: FETCH_SIZE, offset },
        channel,
      ))
      if (mine === generation) {
        durationMs.value = summary.durationMs ?? 0
        fetchedAll.value = (summary.rows ?? 0) < FETCH_SIZE
      }
    }
    catch (error) {
      if (mine === generation)
        toast.error(error instanceof Error ? error.message : String(error))
    }
    finally {
      if (mine === generation)
        loading.value = false
    }
  }

  return { columns, rows, durationMs, loading, fetchedAll, fetchRows }
}
