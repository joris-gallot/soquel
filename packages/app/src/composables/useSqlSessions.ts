import type { QueryResult } from '@/lib/bindings'
import { ref } from 'vue'
import { commands } from '@/lib/bindings'
import { CommandError, unwrap } from '@/lib/result'

// One pinned backend session per editor tab: `${connectionId}:${tabId}`.
const sessionIds = ref<Record<string, string>>({})

export function useSqlSessions() {
  async function ensure(connectionId: string, tabId: string): Promise<string> {
    const key = `${connectionId}:${tabId}`
    const existing = sessionIds.value[key]
    if (existing)
      return existing
    const id = unwrap(await commands.openSqlSession(connectionId))
    sessionIds.value[key] = id
    return id
  }

  async function run(connectionId: string, tabId: string, sql: string): Promise<QueryResult> {
    const id = await ensure(connectionId, tabId)
    try {
      return unwrap(await commands.runSessionQuery(id, sql))
    }
    catch (error) {
      // The backend closes sessions on disconnect: reopen once and retry.
      if (error instanceof CommandError && error.kind === 'not-found') {
        delete sessionIds.value[`${connectionId}:${tabId}`]
        return unwrap(await commands.runSessionQuery(await ensure(connectionId, tabId), sql))
      }
      throw error
    }
  }

  async function cancel(connectionId: string, tabId: string) {
    const id = sessionIds.value[`${connectionId}:${tabId}`]
    if (id)
      unwrap(await commands.cancelSessionQuery(id))
  }

  /// Frees the pinned client when its tab closes.
  async function close(connectionId: string, tabId: string) {
    const key = `${connectionId}:${tabId}`
    const id = sessionIds.value[key]
    delete sessionIds.value[key]
    if (id)
      unwrap(await commands.closeSqlSession(id))
  }

  function evictConnection(connectionId: string) {
    for (const key of Object.keys(sessionIds.value)) {
      if (key.startsWith(`${connectionId}:`))
        delete sessionIds.value[key]
    }
  }

  return { run, cancel, close, evictConnection }
}
