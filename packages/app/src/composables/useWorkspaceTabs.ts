import type { Ref } from 'vue'
import type { ColumnFilter } from '@/lib/bindings'
import type { TabsState } from '@/lib/tabs'
import { useLocalStorage } from '@vueuse/core'
import { activateSibling, closeTab, EMPTY_TABS, openSqlTab, openTableTab } from '@/lib/tabs'

const states = new Map<string, Ref<TabsState>>()

/// Open tabs survive a restart (sql drafts are keyed by tab id).
export function useWorkspaceTabs(connectionId: string) {
  let state = states.get(connectionId)
  if (!state) {
    state = useLocalStorage<TabsState>(`soquel:tabs:${connectionId}`, EMPTY_TABS, {
      writeDefaults: false,
    })
    states.set(connectionId, state)
  }

  return {
    state,
    openTable: (schema: string, table: string, filters?: ColumnFilter[]) =>
      (state.value = openTableTab(state.value, schema, table, filters)),
    openSql: () => (state.value = openSqlTab(state.value)),
    close: (id: string) => (state.value = closeTab(state.value, id)),
    activate: (id: string) => (state.value = { ...state.value, activeId: id }),
    cycle: (direction: 1 | -1) => (state.value = activateSibling(state.value, direction)),
  }
}
