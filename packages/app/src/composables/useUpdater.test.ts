import type { UpdateProgress } from '@/lib/bindings'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useUpdater } from './useUpdater'

let emitProgress: ((event: { payload: UpdateProgress }) => void) | null = null

const update = {
  version: '0.1.1',
  currentVersion: '0.1.0',
  notes: 'Fixes',
  pubDate: '2026-08-02T12:00:00Z',
}

const state = {
  found: update as typeof update | null,
  checkFails: false,
  installFails: false,
}

vi.mock('@/lib/bindings', () => ({
  commands: {
    checkUpdate: vi.fn(async () =>
      state.checkFails
        ? { status: 'error', error: { kind: 'update', message: 'endpoint unreachable' } }
        : { status: 'ok', data: state.found },
    ),
    installUpdate: vi.fn(async () =>
      state.installFails
        ? { status: 'error', error: { kind: 'update', message: 'signature mismatch' } }
        : { status: 'ok', data: null },
    ),
  },
  events: {
    updateProgress: {
      listen: vi.fn(async (handler: (event: { payload: UpdateProgress }) => void) => {
        emitProgress = handler
        return () => {}
      }),
    },
  },
}))

describe('useUpdater', () => {
  beforeEach(() => {
    state.found = update
    state.checkFails = false
    state.installFails = false
    const { available, downloading, downloaded, total } = useUpdater()
    available.value = null
    downloading.value = false
    downloaded.value = 0
    total.value = null
  })

  it('reports an available update', async () => {
    const { check, available } = useUpdater()

    expect(await check()).toBe(true)
    expect(available.value?.version).toBe('0.1.1')
  })

  it('swallows a failed check: an unreachable endpoint is the offline case', async () => {
    state.checkFails = true
    const { check, available } = useUpdater()

    expect(await check()).toBe(false)
    expect(available.value).toBeNull()
  })

  it('clears a previously found update when the next check comes back empty', async () => {
    const { check, available } = useUpdater()
    await check()
    state.found = null

    expect(await check()).toBe(false)
    expect(available.value).toBeNull()
  })

  it('stops downloading when the install fails', async () => {
    state.installFails = true
    const { install, downloading } = useUpdater()

    await expect(install()).rejects.toThrow('signature mismatch')
    expect(downloading.value).toBe(false)
  })

  it('has no progress ratio until the download reports a total', () => {
    const { downloading, downloaded, total, progress } = useUpdater()
    downloading.value = true

    expect(progress.value).toBeNull()

    total.value = 400
    downloaded.value = 100
    expect(progress.value).toBe(0.25)
  })

  it('switches to installing once the last chunk landed', () => {
    const { downloading, downloaded, total, installing } = useUpdater()
    downloading.value = true
    total.value = 400

    downloaded.value = 399
    expect(installing.value).toBe(false)

    downloaded.value = 400
    expect(installing.value).toBe(true)
  })

  // specta renders the core's f64 as `number | null`, so the event can carry
  // null where Rust promised a number.
  it('reads a null byte count as zero rather than NaN', async () => {
    const { listen, downloaded, total } = useUpdater()
    await listen()

    emitProgress!({ payload: { downloaded: null, total: null } })
    expect(downloaded.value).toBe(0)

    emitProgress!({ payload: { downloaded: 2048, total: 4096 } })
    expect(downloaded.value).toBe(2048)
    expect(total.value).toBe(4096)
  })

  it('never rewinds the bar on an out-of-order event', async () => {
    const { listen, downloaded } = useUpdater()
    await listen()

    emitProgress!({ payload: { downloaded: 3000, total: 4096 } })
    emitProgress!({ payload: { downloaded: 2000, total: 4096 } })

    expect(downloaded.value).toBe(3000)
  })

  it('is not installing while the total is still unknown', () => {
    const { downloading, downloaded, installing } = useUpdater()
    downloading.value = true
    downloaded.value = 400

    expect(installing.value).toBe(false)
  })
})
