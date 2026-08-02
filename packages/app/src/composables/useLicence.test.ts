import { describe, expect, it, vi } from 'vitest'
import { FREE_TABS } from '@/lib/tabs'
import { useLicence } from './useLicence'

const licenceStatus = vi.fn()
const installLicence = vi.fn()

vi.mock('@/lib/bindings', () => ({
  commands: {
    licenceStatus: () => licenceStatus(),
    installLicence: (token: string) => installLicence(token),
  },
}))

describe('useLicence', () => {
  it('keeps the app on the free tier when the licence cannot be read', async () => {
    licenceStatus.mockResolvedValue({
      status: 'error',
      error: { kind: 'secret', message: 'permission denied' },
    })
    const { load, status, tabLimit } = useLicence()

    // A licence file that will not open is a paying customer with a broken
    // install; refusing to start would be worse than the limit.
    await expect(load()).resolves.toBeUndefined()

    expect(status.value.kind).toBe('free')
    expect(tabLimit.value).toBe(FREE_TABS)
  })

  it('lifts the tab limit once a licence is in', async () => {
    installLicence.mockResolvedValue({
      status: 'ok',
      data: { kind: 'licensed', email: 'a@b.c', name: null, updatesUntil: '2027-01-01T00:00:00Z' },
    })
    const { install, tabLimit } = useLicence()

    await install('token')

    expect(tabLimit.value).toBe(Number.POSITIVE_INFINITY)
  })

  it('throws when a licence is refused, unlike the read at startup', async () => {
    installLicence.mockResolvedValue({
      status: 'error',
      error: { kind: 'secret', message: 'this licence looks cut short' },
    })
    const { install } = useLicence()

    // The dialog has to say why: pasting a licence is a deliberate action.
    await expect(install('rubbish')).rejects.toThrow('this licence looks cut short')
  })
})
