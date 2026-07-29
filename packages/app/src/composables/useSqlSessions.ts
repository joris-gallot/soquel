import type { QueryResult } from '@/lib/bindings'
import { ref } from 'vue'
import { commands } from '@/lib/bindings'
import { CommandError, unwrap } from '@/lib/result'

const sessionIds = ref<Record<string, string>>({})

export function useSqlSessions() {
  async function ensure(connectionId: string): Promise<string> {
    const existing = sessionIds.value[connectionId]
    if (existing)
      return existing
    const id = unwrap(await commands.openSqlSession(connectionId))
    sessionIds.value[connectionId] = id
    return id
  }

  async function run(connectionId: string, sql: string): Promise<QueryResult> {
    const id = await ensure(connectionId)
    try {
      return unwrap(await commands.runSessionQuery(id, sql))
    }
    catch (error) {
      // The backend closes sessions on disconnect: reopen once and retry.
      if (error instanceof CommandError && error.kind === 'not-found') {
        evict(connectionId)
        return unwrap(await commands.runSessionQuery(await ensure(connectionId), sql))
      }
      throw error
    }
  }

  async function cancel(connectionId: string) {
    const id = sessionIds.value[connectionId]
    if (id)
      unwrap(await commands.cancelSessionQuery(id))
  }

  function evict(connectionId: string) {
    delete sessionIds.value[connectionId]
  }

  return { run, cancel, evict }
}
