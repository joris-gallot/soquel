import { $, browser } from '@wdio/globals'
import { clickVisible, createConnection, deleteFirstConnection, typeSql, waitForText } from './helpers'

describe('mysql workspace', () => {
  it('creates a mysql connection and opens its schema', async () => {
    await $('[data-testid="empty-state"]').waitForExist()
    await createConnection('fixture mysql', 'mysql')

    await $('[data-testid="open-fixture mysql"]').click()
    await waitForText('[data-testid="workspace-name"]', 'fixture mysql')
    await waitForText('[data-testid="server-version"]', 'MySQL')
    await $('[data-testid="table-soquel_test.customers"]').waitForExist({ timeout: 10_000 })
    await browser.saveScreenshot('./e2e/screenshots/mysql-workspace.png')
  })

  it('browses and edits a table through the shared grid', async () => {
    await $('[data-testid="table-soquel_test.customers"]').click()
    await waitForText('[data-testid="table-title"]', 'soquel_test.customers')
    await waitForText('[data-testid="grid-body"]', 'ada@example.com')
    // NULL email from the fixture renders like on postgres.
    await waitForText('[data-testid="grid-body"]', 'NULL')

    await clickVisible('[data-testid="sort-name"]')
    await waitForText('[data-testid="grid-body"] tr:first-child', 'Ada Lovelace')
    await browser.saveScreenshot('./e2e/screenshots/mysql-grid.png')
  })

  it('shows the ddl from show create table', async () => {
    await clickVisible('[data-testid="grid-view-ddl"]')
    await waitForText('[data-testid="ddl-view"]', 'CREATE TABLE `customers`')
    await clickVisible('[data-testid="grid-view-data"]')
  })

  it('runs sql and renders a mysql explain tree', async () => {
    await typeSql('select count(*) as n from customers')
    await clickVisible('[data-testid="run-query"]')
    await waitForText('[data-testid="sql-results"]', 'n')

    await typeSql('select o.id, c.name from orders o join customers c on c.id = o.customer_id')
    await clickVisible('[data-testid="explain-menu"]')
    await $('[data-testid="explain-plain"]').click()
    await waitForText('[data-testid="explain-tree"]', 'Nested loop')
    await waitForText('[data-testid="explain-tree"]', 'estimated costs only')
    await browser.saveScreenshot('./e2e/screenshots/mysql-explain.png')
  })

  it('disconnects and cleans up', async () => {
    await $('[data-testid="workspace-disconnect"]').click()
    await $('[data-testid="connection-row"]').waitForExist()
    await deleteFirstConnection()
  })
})
