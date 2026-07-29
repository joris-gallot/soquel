import type { Ref } from 'vue'
import { useElementSize, useScroll } from '@vueuse/core'
import { computed, ref, watchEffect } from 'vue'
import { windowFor } from '@/lib/virtual'

const FALLBACK_ROW_HEIGHT = 25

/// Fixed-height row virtualization over an existing scroll container.
/// Render `window.start..window.end` between two spacer rows.
export function useVirtualRows(container: Ref<HTMLElement | null>, rowCount: Ref<number>) {
  const { y } = useScroll(container)
  const { height } = useElementSize(container)
  const rowHeight = ref(FALLBACK_ROW_HEIGHT)

  // Measure the first real row once it exists; tolerate zero (hidden tab).
  watchEffect(() => {
    if (rowCount.value === 0)
      return
    const row = container.value?.querySelector('tbody tr[data-row]')
    const measured = row instanceof HTMLElement ? row.offsetHeight : 0
    if (measured > 0)
      rowHeight.value = measured
  })

  return {
    rowHeight,
    window: computed(() => windowFor(y.value, height.value, rowHeight.value, rowCount.value)),
  }
}
