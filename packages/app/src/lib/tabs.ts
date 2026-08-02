import type { ColumnFilter } from '@/lib/bindings'

export type WorkspaceTab
  = | { id: string, type: 'table', schema: string, table: string, initialFilters?: ColumnFilter[] }
    | { id: string, type: 'sql', title: string }

export interface TabsState {
  tabs: WorkspaceTab[]
  activeId: string | null
}

export const EMPTY_TABS: TabsState = { tabs: [], activeId: null }

/// What the free tier allows per connection. Infinity once a licence unlocks it.
export const FREE_TABS = 2

/// Activates the existing tab for that table (replacing its initial filters),
/// or appends a new one.
export function openTableTab(
  state: TabsState,
  schema: string,
  table: string,
  filters?: ColumnFilter[],
  limit = Number.POSITIVE_INFINITY,
): TabsState {
  const existing = state.tabs.find(
    tab => tab.type === 'table' && tab.schema === schema && tab.table === table,
  )
  if (existing) {
    return {
      tabs: state.tabs.map(tab =>
        tab === existing ? { ...existing, initialFilters: filters } : tab,
      ),
      activeId: existing.id,
    }
  }
  // Only opening is limited. Re-activating a tab that is already there adds
  // nothing, and refusing it would block navigation instead of a purchase.
  if (state.tabs.length >= limit)
    return state
  const tab: WorkspaceTab = {
    id: crypto.randomUUID(),
    type: 'table',
    schema,
    table,
    initialFilters: filters,
  }
  return { tabs: [...state.tabs, tab], activeId: tab.id }
}

export function openSqlTab(state: TabsState, limit = Number.POSITIVE_INFINITY): TabsState {
  if (state.tabs.length >= limit)
    return state
  const taken = state.tabs
    .filter(tab => tab.type === 'sql')
    .map(tab => Number.parseInt(tab.title.replace('sql ', ''), 10))
    .filter(Number.isFinite)
  const number = taken.length === 0 ? 1 : Math.max(...taken) + 1
  const tab: WorkspaceTab = { id: crypto.randomUUID(), type: 'sql', title: `sql ${number}` }
  return { tabs: [...state.tabs, tab], activeId: tab.id }
}

/// Next active tab = right neighbor, else left, else none.
export function closeTab(state: TabsState, id: string): TabsState {
  const index = state.tabs.findIndex(tab => tab.id === id)
  if (index === -1)
    return state
  const tabs = state.tabs.filter(tab => tab.id !== id)
  if (state.activeId !== id)
    return { tabs, activeId: state.activeId }
  const next = tabs[index] ?? tabs[index - 1] ?? null
  return { tabs, activeId: next?.id ?? null }
}

export function activateSibling(state: TabsState, direction: 1 | -1): TabsState {
  if (state.tabs.length === 0)
    return state
  const index = state.tabs.findIndex(tab => tab.id === state.activeId)
  const next = (index + direction + state.tabs.length) % state.tabs.length
  return { ...state, activeId: state.tabs[next].id }
}
