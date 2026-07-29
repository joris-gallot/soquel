export interface VirtualWindow {
  start: number
  end: number
  padTop: number
  padBottom: number
}

/// Slice of rows worth rendering for a scroll position, with spacer heights.
export function windowFor(
  scrollTop: number,
  viewportHeight: number,
  rowHeight: number,
  rowCount: number,
  overscan = 10,
): VirtualWindow {
  // Clamped to the list: a stale scroll position (e.g. right after a reset)
  // must not produce an empty window.
  const start = Math.min(Math.max(0, Math.floor(scrollTop / rowHeight) - overscan), rowCount)
  const visible = Math.ceil(viewportHeight / rowHeight) + overscan * 2
  const end = Math.min(rowCount, start + visible)
  return {
    start,
    end,
    padTop: start * rowHeight,
    padBottom: Math.max(0, (rowCount - end) * rowHeight),
  }
}
