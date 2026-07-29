import { $, $$, browser, expect } from '@wdio/globals'
import { createPostgresConnection, deleteFirstConnection, typeSql, waitForText } from './helpers'

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
    await $('[data-testid="sort-name"]').click()
    await waitForText('[data-testid="grid-body"] tr:first-child', 'Ada Lovelace')

    await $('[data-testid="sort-name"]').click()
    await waitForText('[data-testid="grid-body"] tr:first-child', 'Grace Hopper')
    await browser.saveScreenshot('./e2e/screenshots/workspace-grid-sorted.png')
  })

  it('filters rows from the header', async () => {
    await $('[data-testid="filter-name"]').click()
    await $('[data-testid="filter-op"]').click()
    await $('[role="option"]*=contains').click()
    await $('[data-testid="filter-value"]').setValue('ada')
    await $('[data-testid="filter-apply"]').click()

    await waitForText('[data-testid="filter-chips"]', 'name contains ada')
    await browser.waitUntil(
      async () => await $$('[data-testid="grid-body"] tr').length === 1,
      { timeoutMsg: 'the filter never narrowed to one row' },
    )
    await waitForText('[data-testid="grid-range"]', 'filtered')
    await browser.saveScreenshot('./e2e/screenshots/workspace-grid-filtered.png')

    await $('[data-testid="clear-filters"]').click()
    await browser.waitUntil(
      async () => await $$('[data-testid="grid-body"] tr').length === 3,
      { timeoutMsg: 'clearing filters never restored the rows' },
    )
  })

  it('inspects a cell', async () => {
    await $('td*=plan').click()
    await $('[data-testid="cell-inspector"]').waitForExist()
    await waitForText('[data-testid="cell-inspector"]', 'jsonb')
    // Pretty-printed json: the key sits alone on an indented line.
    await waitForText('[data-testid="inspector-value"]', '  "plan"')
    await browser.saveScreenshot('./e2e/screenshots/workspace-inspector.png')
    await $('[data-testid="inspector-close"]').click()
    await $('[data-testid="cell-inspector"]').waitForExist({ reverse: true })
  })

  it('hops along a foreign key', async () => {
    await $('[data-testid="tree-filter"]').setValue('orders')
    await $('[data-testid="table-app.orders"]').click()
    await waitForText('[data-testid="table-title"]', 'app.orders')

    // The hover-only cell button is not clickable headlessly: hop from the inspector.
    await $('[data-testid="grid-body"] tr:first-child td:nth-child(2)').click()
    await $('[data-testid="inspector-hop"]').click()

    await waitForText('[data-testid="table-title"]', 'app.customers')
    await waitForText('[data-testid="filter-chips"]', 'id = 1')
    await browser.waitUntil(
      async () => await $$('[data-testid="grid-body"] tr').length === 1,
      { timeoutMsg: 'the hop never narrowed to the referenced row' },
    )
    await waitForText('[data-testid="grid-body"]', 'Ada Lovelace')
    await $('[data-testid="clear-filters"]').click()
  })

  it('runs sql in the editor', async () => {
    await $('[data-testid="view-sql"]').click()
    await $('[data-testid="sql-editor"]').waitForExist()

    await typeSql('select count(*) as customer_count from app.customers')
    await $('[data-testid="run-query"]').click()

    await waitForText('[data-testid="sql-results"]', 'customer_count')
    await waitForText('[data-testid="sql-results"]', '3')
    await browser.saveScreenshot('./e2e/screenshots/workspace-sql.png')

    // Back to the data view: grid state survived.
    await $('[data-testid="view-data"]').click()
    await waitForText('[data-testid="table-title"]', 'app.customers')
  })

  it('shows a database error and keeps the session usable', async () => {
    await typeSql('select * from app.nope')
    await $('[data-testid="run-query"]').click()
    await waitForText('[data-testid="sql-error"]', 'does not exist')

    // The pinned session survives a failed statement.
    await typeSql('select 42 as answer')
    await $('[data-testid="run-query"]').click()
    await waitForText('[data-testid="sql-results"]', 'answer')
  })

  it('splits a multi-statement script into result tabs', async () => {
    await typeSql('select 1 as first; select 2 as second')
    await $('[data-testid="run-query"]').click()

    const tabs = await $$('[data-testid="sql-results"] [role="tab"]')
    expect(tabs).toHaveLength(2)
    await waitForText('[data-testid="sql-results"]', 'first')

    await tabs[1].click()
    await waitForText('[data-testid="sql-results"]', 'second')
  })

  it('reloads a statement from history', async () => {
    await $('[data-testid="view-sql"]').click()
    await $('[data-testid="query-history"]').click()
    await $('[data-testid="history-list"]').waitForExist()

    // Several statements ran before this: search narrows to the one we want.
    await $('[data-testid="history-search"]').setValue('customer_count')
    await waitForText('[data-testid="history-list"]', 'customer_count')

    await $('[data-testid="history-list"] [data-testid="history-item"]').click()
    await $('[data-testid="history-list"]').waitForExist({ reverse: true })
    await waitForText('[data-testid="sql-input"]', 'customer_count')
    await browser.saveScreenshot('./e2e/screenshots/workspace-sql-history.png')
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
