import type { TabsState } from './tabs'
import { describe, expect, it } from 'vitest'
import { activateSibling, closeTab, EMPTY_TABS, FREE_TABS, openSqlTab, openTableTab } from './tabs'

function tableTab(state: TabsState, index: number) {
  const tab = state.tabs[index]
  if (tab.type !== 'table')
    throw new Error('expected a table tab')
  return tab
}

describe('openTableTab', () => {
  it('appends and activates a new table tab', () => {
    const state = openTableTab(EMPTY_TABS, 'app', 'customers')
    expect(state.tabs).toHaveLength(1)
    expect(state.activeId).toBe(state.tabs[0].id)
  })

  it('activates the existing tab and replaces its filters instead of duplicating', () => {
    let state = openTableTab(EMPTY_TABS, 'app', 'customers')
    state = openTableTab(state, 'app', 'orders')
    state = openTableTab(state, 'app', 'customers', [{ column: 'id', op: 'eq', value: '1' }])

    expect(state.tabs).toHaveLength(2)
    expect(state.activeId).toBe(state.tabs[0].id)
    expect(tableTab(state, 0).initialFilters).toEqual([{ column: 'id', op: 'eq', value: '1' }])
  })
})

describe('openSqlTab', () => {
  it('numbers editors past the highest existing one', () => {
    let state = openSqlTab(EMPTY_TABS)
    state = openSqlTab(state)
    expect(state.tabs.map(tab => tab.type === 'sql' && tab.title)).toEqual(['sql 1', 'sql 2'])

    state = closeTab(state, state.tabs[0].id)
    state = openSqlTab(state)
    // "sql 2" is still open: the next editor must not reuse its number.
    expect(state.tabs.map(tab => tab.type === 'sql' && tab.title)).toEqual(['sql 2', 'sql 3'])
  })
})

describe('closeTab', () => {
  function three(): TabsState {
    let state = openTableTab(EMPTY_TABS, 'app', 'a')
    state = openTableTab(state, 'app', 'b')
    state = openTableTab(state, 'app', 'c')
    return state
  }

  it('activates the right neighbor, then the left one at the end', () => {
    let state = three()
    state = { ...state, activeId: state.tabs[1].id }
    state = closeTab(state, state.tabs[1].id)
    expect(state.activeId).toBe(state.tabs[1].id) // was "c", shifted into slot 1

    state = { ...state, activeId: state.tabs[1].id }
    state = closeTab(state, state.tabs[1].id)
    expect(state.activeId).toBe(state.tabs[0].id)
  })

  it('keeps the active tab when closing another one', () => {
    let state = three()
    const active = state.activeId
    state = closeTab(state, state.tabs[0].id)
    expect(state.activeId).toBe(active)
  })

  it('empties cleanly', () => {
    let state = openTableTab(EMPTY_TABS, 'app', 'a')
    state = closeTab(state, state.tabs[0].id)
    expect(state).toEqual({ tabs: [], activeId: null })
  })
})

describe('activateSibling', () => {
  it('cycles in both directions and wraps', () => {
    let state = openTableTab(EMPTY_TABS, 'app', 'a')
    state = openTableTab(state, 'app', 'b')

    state = activateSibling(state, 1)
    expect(state.activeId).toBe(state.tabs[0].id)
    state = activateSibling(state, -1)
    expect(state.activeId).toBe(state.tabs[1].id)
  })
})

describe('the free tier limit', () => {
  const full = openSqlTab(openSqlTab(EMPTY_TABS, FREE_TABS), FREE_TABS)

  it('refuses a third tab, of either kind', () => {
    expect(full.tabs).toHaveLength(2)
    expect(openSqlTab(full, FREE_TABS)).toBe(full)
    expect(openTableTab(full, 'public', 'orders', undefined, FREE_TABS)).toBe(full)
  })

  it('still activates a tab that is already open', () => {
    // Re-activating opens nothing. Refusing it would block navigation between
    // the tabs someone already has, which is not what the limit is for.
    const opened = openTableTab(EMPTY_TABS, 'public', 'users', undefined, FREE_TABS)
    const two = openSqlTab(opened, FREE_TABS)
    expect(two.tabs).toHaveLength(2)

    const back = openTableTab(two, 'public', 'users', undefined, FREE_TABS)

    expect(back.tabs).toHaveLength(2)
    expect(back.activeId).toBe(opened.activeId)
  })

  it('opens without limit when none is given', () => {
    let state = EMPTY_TABS
    for (let i = 0; i < 5; i++)
      state = openSqlTab(state)
    expect(state.tabs).toHaveLength(5)
  })
})
