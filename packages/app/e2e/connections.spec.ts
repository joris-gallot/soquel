import { $, browser } from '@wdio/globals'

const HELMO_PG = { host: 'localhost', port: '5440', database: 'helmo', user: 'helmo', password: 'helmo' }

describe('connection manager', () => {
  it('starts on the empty state', async () => {
    await $('[data-testid="empty-state"]').waitForExist()
    await browser.saveScreenshot('./e2e/screenshots/connections-empty.png')
  })

  it('creates a connection after a successful test', async () => {
    await $('[data-testid="new-connection"]').click()
    await $('[data-testid="field-name"]').setValue('helmo local')
    await $('[data-testid="field-host"]').setValue(HELMO_PG.host)
    await $('[data-testid="field-port"]').setValue(HELMO_PG.port)
    await $('[data-testid="field-database"]').setValue(HELMO_PG.database)
    await $('[data-testid="field-user"]').setValue(HELMO_PG.user)
    await $('[data-testid="field-password"]').setValue(HELMO_PG.password)

    await $('[data-testid="test-connection"]').click()
    await browser.waitUntil(
      async () => (await $('[data-testid="test-result"]').getText()).includes('Connection OK'),
      { timeout: 15_000, timeoutMsg: 'test connection never succeeded' },
    )
    await browser.saveScreenshot('./e2e/screenshots/connections-form.png')

    await $('[data-testid="save-connection"]').click()
    await $('[data-testid="connection-row"]').waitForExist()
  })

  it('connects and disconnects', async () => {
    await $('[data-testid="toggle-connection"]').click()
    await $('[data-testid="status-connected"]').waitForExist()
    await browser.saveScreenshot('./e2e/screenshots/connections-connected.png')

    await $('[data-testid="toggle-connection"]').click()
    await $('[data-testid="status-disconnected"]').waitForExist()
  })

  it('deletes the connection', async () => {
    await $('[data-testid="row-menu"]').click()
    await $('[data-testid="row-delete"]').click()
    await $('[data-testid="empty-state"]').waitForExist()
  })
})
