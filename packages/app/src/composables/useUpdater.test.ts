import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useUpdater } from './useUpdater'

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
  events: { updateProgress: { listen: vi.fn(async () => () => {}) } },
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
})
