import type { Ref } from 'vue'
import type { ColumnFilter } from '@/lib/bindings'
import type { TabsState } from '@/lib/tabs'
import { useLocalStorage } from '@vueuse/core'
import { toast } from 'vue-sonner'
import { useLicence } from '@/composables/useLicence'
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
  const held = state
  const { tabLimit, panelOpen } = useLicence()

  /// The pure functions return the state untouched when the limit is reached,
  /// which is the only signal there is that nothing opened.
  function apply(next: TabsState) {
    if (next === held.value) {
      toast('Two tabs at a time on the free tier.', {
        description: 'Close one, or unlock the app to open as many as you like.',
        action: { label: 'Unlock', onClick: () => (panelOpen.value = true) },
        // Stacked: inline, the button eats the middle of the sentence.
        classes: { toast: 'flex-col items-start gap-2' },
      })
      return
    }
    held.value = next
  }

  return {
    state,
    openTable: (schema: string, table: string, filters?: ColumnFilter[]) =>
      apply(openTableTab(held.value, schema, table, filters, tabLimit.value)),
    openSql: () => apply(openSqlTab(held.value, tabLimit.value)),
    close: (id: string) => (held.value = closeTab(held.value, id)),
    activate: (id: string) => (held.value = { ...held.value, activeId: id }),
    cycle: (direction: 1 | -1) => (held.value = activateSibling(held.value, direction)),
  }
}
