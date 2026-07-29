import { $, browser } from '@wdio/globals'
import { TEST_DBS } from './fixtures'
import { setEditorValue, waitForText } from './helpers'

const PG = TEST_DBS.postgres

describe('connection manager', () => {
  it('starts on the empty state', async () => {
    await $('[data-testid="empty-state"]').waitForExist()
    await browser.saveScreenshot('./e2e/screenshots/connections-empty.png')
  })

  it('creates a connection after a successful test', async () => {
    await $('[data-testid="new-connection"]').click()
    await $('[data-testid="field-name"]').setValue('test pg')
    await $('[data-testid="field-host"]').setValue(PG.host)
    await $('[data-testid="field-port"]').setValue(PG.port)
    await $('[data-testid="field-database"]').setValue(PG.database)
    await $('[data-testid="field-user"]').setValue(PG.user)
    await $('[data-testid="field-password"]').setValue(PG.password)

    await $('[data-testid="test-connection"]').click()
    await browser.waitUntil(
      async () => (await $('[data-testid="test-result"]').getText()).includes('Connection OK'),
      { timeout: 15_000, timeoutMsg: 'test connection never succeeded' },
    )
    await browser.saveScreenshot('./e2e/screenshots/connections-form.png')

    await $('[data-testid="save-connection"]').click()
    await $('[data-testid="connection-row"]').waitForExist()
    await $('[data-testid="field-name"]').waitForExist({ reverse: true })
  })

  it('connects into the workspace, then disconnects from the list', async () => {
    await $('[data-testid="toggle-connection"]').click()
    await waitForText('[data-testid="workspace-name"]', 'test pg')

    await $('[data-testid="workspace-back"]').click()
    await $('[data-testid="status-connected"]').waitForExist()
    await browser.saveScreenshot('./e2e/screenshots/connections-connected.png')

    await $('[data-testid="toggle-connection"]').click()
    await $('[data-testid="status-disconnected"]').waitForExist()
  })

  it('groups connections into collapsible sections', async () => {
    await $('[data-testid="row-menu"]').click()
    await $('[data-testid="row-edit"]').click()
    await setEditorValue('[data-testid="field-group"]', 'clients')
    await $('[data-testid="save-connection"]').click()
    await $('[data-testid="field-group"]').waitForExist({ reverse: true })

    await waitForText('[data-testid="group-clients"]', 'clients (1)')
    await $('[data-testid="connection-row"]').waitForExist()

    await $('[data-testid="group-clients"]').click()
    await $('[data-testid="connection-row"]').waitForExist({ reverse: true })
    await $('[data-testid="group-clients"]').click()
    await $('[data-testid="connection-row"]').waitForExist()
  })

  it('deletes the connection', async () => {
    await $('[data-testid="row-menu"]').click()
    await $('[data-testid="row-delete"]').click()
    await $('[data-testid="empty-state"]').waitForExist()
  })
})
