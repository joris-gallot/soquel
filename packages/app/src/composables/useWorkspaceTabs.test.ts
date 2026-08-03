import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { FREE_TABS } from '@/lib/tabs'

const toast = vi.fn()
const tabLimit = ref<number>(FREE_TABS)
const panelOpen = ref(false)

// A plain ref: useLocalStorage wants a browser, and this suite runs in node.
vi.mock('@vueuse/core', () => ({
  useLocalStorage: (_key: string, initial: unknown) => ref(structuredClone(initial)),
}))
vi.mock('vue-sonner', () => ({ toast: (...args: unknown[]) => toast(...args) }))
vi.mock('@/composables/useLicence', () => ({
  useLicence: () => ({ tabLimit, panelOpen }),
}))

async function freshTabs(connectionId: string) {
  // The composable holds one state per connection at module scope.
  vi.resetModules()
  const { useWorkspaceTabs } = await import('./useWorkspaceTabs')
  return useWorkspaceTabs(connectionId)
}

beforeEach(() => {
  toast.mockClear()
  tabLimit.value = FREE_TABS
  panelOpen.value = false
})

describe('useWorkspaceTabs against the free tier', () => {
  it('refuses the tab past the limit and says why', async () => {
    const { state, openTable, openSql } = await freshTabs('c-1')

    openTable('public', 'users')
    openSql()
    openTable('public', 'orders')

    // The pure functions return the state untouched, and this is the only place
    // that turns silence into something the user can act on.
    expect(state.value.tabs).toHaveLength(FREE_TABS)
    expect(toast).toHaveBeenCalledOnce()
    expect(toast.mock.calls[0]?.[0]).toContain('free tier')
  })

  it('offers the way out rather than only the bad news', async () => {
    const { openSql, openTable } = await freshTabs('c-2')
    openTable('public', 'users')
    openSql()
    toast.mockClear()

    openSql()

    const options = toast.mock.calls[0]?.[1] as { action: { onClick: () => void } }
    options.action.onClick()
    // A refusal that does not open the licence dialog is a dead end.
    expect(panelOpen.value).toBe(true)
  })

  it('lets an already open tab be reactivated at the limit', async () => {
    const { state, openTable, openSql } = await freshTabs('c-3')
    openTable('public', 'users')
    openSql()
    const sqlId = state.value.activeId

    openTable('public', 'users')

    // Navigating back to a tab someone already has must never read as a purchase
    // prompt: the limit is on opening, not on moving around.
    expect(state.value.tabs).toHaveLength(FREE_TABS)
    expect(state.value.activeId).not.toBe(sqlId)
    expect(toast).not.toHaveBeenCalled()
  })

  it('opens past two once the limit is lifted', async () => {
    tabLimit.value = Number.POSITIVE_INFINITY
    const { state, openTable, openSql } = await freshTabs('c-4')

    openTable('public', 'users')
    openSql()
    openSql()

    expect(state.value.tabs).toHaveLength(3)
    expect(toast).not.toHaveBeenCalled()
  })
})
