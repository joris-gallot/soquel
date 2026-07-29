import { $, browser } from '@wdio/globals'
import { createPostgresConnection, deleteFirstConnection, waitForText } from './helpers'

describe('workspace', () => {
  it('opens a workspace with the schema tree after connecting', async () => {
    await $('[data-testid="empty-state"]').waitForExist()
    await createPostgresConnection('fixture db')

    await $('[data-testid="open-fixture db"]').click()
    await waitForText('[data-testid="workspace-name"]', 'fixture db')
    await $('[data-testid="table-app.customers"]').waitForExist({ timeout: 10_000 })
    await $('[data-testid="table-public.settings"]').waitForExist()
    await browser.saveScreenshot('./e2e/screenshots/workspace.png')
  })

  it('filters the tree and opens a table in the grid', async () => {
    await $('[data-testid="tree-filter"]').setValue('cust')
    await $('[data-testid="table-app.orders"]').waitForExist({ reverse: true })

    await $('[data-testid="table-app.customers"]').click()
    await waitForText('[data-testid="table-title"]', 'app.customers')
    await waitForText('[data-testid="grid-body"]', 'ada@example.com')
    // Grace Hopper has a NULL email in the fixture.
    await waitForText('[data-testid="grid-body"]', 'NULL')
    await browser.saveScreenshot('./e2e/screenshots/workspace-grid.png')
  })

  it('sorts by column from the header', async () => {
    await $('[data-testid="grid-header-name"]').click()
    await waitForText('[data-testid="grid-body"] tr:first-child', 'Ada Lovelace')

    await $('[data-testid="grid-header-name"]').click()
    await waitForText('[data-testid="grid-body"] tr:first-child', 'Grace Hopper')
    await browser.saveScreenshot('./e2e/screenshots/workspace-grid-sorted.png')
  })

  it('runs sql in the editor', async () => {
    await $('[data-testid="view-sql"]').click()
    await $('[data-testid="sql-editor"]').waitForExist()

    await $('[data-testid="sql-input"] .cm-content').click()
    // Single insertText event: per-key typing drops keystrokes under WebKitWebDriver.
    await browser.execute(() =>
      document.execCommand('insertText', false, 'select count(*) as customer_count from app.customers'))
    await waitForText('[data-testid="sql-input"]', 'customer_count')
    await $('[data-testid="run-query"]').click()

    await waitForText('[data-testid="sql-results"]', 'customer_count')
    await waitForText('[data-testid="sql-results"]', '3')
    await browser.saveScreenshot('./e2e/screenshots/workspace-sql.png')

    // Back to the data view: grid state survived.
    await $('[data-testid="view-data"]').click()
    await waitForText('[data-testid="table-title"]', 'app.customers')
  })

  it('opens the command palette', async () => {
    await $('[data-testid="open-palette"]').click()
    await $('[data-testid="palette-input"]').waitForExist()
    await browser.saveScreenshot('./e2e/screenshots/palette.png')
    await browser.keys(['Escape'])
    await $('[data-testid="palette-input"]').waitForExist({ reverse: true })
  })

  it('disconnects and cleans up', async () => {
    await $('[data-testid="workspace-disconnect"]').click()
    await $('[data-testid="connection-row"]').waitForExist()
    await deleteFirstConnection()
  })
})
