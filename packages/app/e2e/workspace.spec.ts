import { $, $$, browser, expect } from '@wdio/globals'
import { beginCellEdit, clickVisible, createPostgresConnection, CTRL, deleteFirstConnection, setEditorValue, typeSql, visible, waitForText, waitForToastsGone, waitForVisibleRows } from './helpers'

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

  it('filters the tree and opens a table in a tab', async () => {
    await $('[data-testid="tree-filter"]').setValue('cust')
    await $('[data-testid="table-app.orders"]').waitForExist({ reverse: true })

    await $('[data-testid="table-app.customers"]').click()
    await $('[data-testid="tab-app.customers"]').waitForExist()
    await waitForText('[data-testid="table-title"]', 'app.customers')
    await waitForText('[data-testid="grid-body"]', 'ada@example.com')
    // Grace Hopper has a NULL email in the fixture.
    await waitForText('[data-testid="grid-body"]', 'NULL')
    await browser.saveScreenshot('./e2e/screenshots/workspace-grid.png')
  })

  it('sorts by column from the header', async () => {
    await clickVisible('[data-testid="sort-name"]')
    await waitForText('[data-testid="grid-body"] tr:first-child', 'Ada Lovelace')

    await clickVisible('[data-testid="sort-name"]')
    await waitForText('[data-testid="grid-body"] tr:first-child', 'Grace Hopper')
    await browser.saveScreenshot('./e2e/screenshots/workspace-grid-sorted.png')
  })

  it('filters rows from the header', async () => {
    await clickVisible('[data-testid="filter-name"]')
    await $('[data-testid="filter-op"]').click()
    await $('[role="option"]*=contains').click()
    await $('[data-testid="filter-value"]').setValue('ada')
    await $('[data-testid="filter-apply"]').click()

    await waitForText('[data-testid="filter-chips"]', 'name contains ada')
    await waitForVisibleRows(1)
    await waitForText('[data-testid="grid-range"]', 'filtered')
    await browser.saveScreenshot('./e2e/screenshots/workspace-grid-filtered.png')

    await clickVisible('[data-testid="clear-filters"]')
    await waitForVisibleRows(3)
  })

  it('inspects a cell', async () => {
    await clickVisible('td*=plan')
    await $('[data-testid="cell-inspector"]').waitForExist()
    await waitForText('[data-testid="cell-inspector"]', 'jsonb')
    // Pretty-printed json: the key sits alone on an indented line.
    await waitForText('[data-testid="inspector-value"]', '  "plan"')
    await browser.saveScreenshot('./e2e/screenshots/workspace-inspector.png')
    await clickVisible('[data-testid="inspector-close"]')
    await $('[data-testid="cell-inspector"]').waitForExist({ reverse: true })
  })

  it('hops along a foreign key into the referenced tab', async () => {
    await $('[data-testid="tree-filter"]').setValue('orders')
    await $('[data-testid="table-app.orders"]').click()
    await $('[data-testid="tab-app.orders"]').waitForExist()
    await waitForText('[data-testid="table-title"]', 'app.orders')

    // The hover-only cell button is not clickable headlessly: hop from the inspector.
    await clickVisible('[data-testid="grid-body"] tr:first-child td:nth-child(2)')
    await clickVisible('[data-testid="inspector-hop"]')

    await waitForText('[data-testid="table-title"]', 'app.customers')
    await waitForText('[data-testid="filter-chips"]', 'id = 1')
    await waitForVisibleRows(1)
    await waitForText('[data-testid="grid-body"]', 'Ada Lovelace')
    await clickVisible('[data-testid="clear-filters"]')
  })

  it('streams a big table and appends on infinite scroll', async () => {
    await $('[data-testid="tree-filter"]').setValue('events')
    await $('[data-testid="table-app.events"]').click()
    await waitForText('[data-testid="table-title"]', 'app.events')
    await waitForText('[data-testid="grid-range"]', '2000+ rows')

    // Virtualization: the DOM holds a window, not the 2000 loaded rows.
    const domRows = await $$('[data-testid="grid-body"] tr[data-row]').length
    expect(domRows).toBeLessThan(200)

    // Reaching the bottom appends the next 2000.
    await browser.execute(() => {
      for (const scroller of document.querySelectorAll('[data-testid="grid-scroller"]')) {
        if (scroller instanceof HTMLElement && scroller.offsetParent !== null)
          scroller.scrollTop = scroller.scrollHeight
      }
    })
    await waitForText('[data-testid="grid-range"]', '4000+ rows')
    await browser.saveScreenshot('./e2e/screenshots/workspace-stream.png')

    await (await visible('[data-testid="close-tab-app.events"]')).click()
    await $('[data-testid="tab-app.events"]').waitForExist({ reverse: true })
  })

  it('dedupes table tabs and keeps state per tab', async () => {
    // Both tables are open from the previous tests: re-clicking activates, no duplicate.
    await $('[data-testid="tree-filter"]').setValue('cust')
    await $('[data-testid="table-app.customers"]').click()
    expect(await $$('[data-testid="tab-bar"] [data-testid^="tab-app."]').length).toBe(2)

    // A filter set in this tab survives switching away and back.
    await clickVisible('[data-testid="filter-name"]')
    await $('[data-testid="filter-op"]').click()
    await $('[role="option"]*=contains').click()
    await $('[data-testid="filter-value"]').setValue('ada')
    await $('[data-testid="filter-apply"]').click()
    await waitForText('[data-testid="filter-chips"]', 'name contains ada')

    await $('[data-testid="tab-app.orders"]').click()
    await waitForText('[data-testid="table-title"]', 'app.orders')
    await $('[data-testid="tab-app.customers"]').click()
    await waitForText('[data-testid="filter-chips"]', 'name contains ada')
    await browser.saveScreenshot('./e2e/screenshots/workspace-tabs.png')
    await clickVisible('[data-testid="clear-filters"]')
  })

  it('closes tabs from the bar and with ctrl+w', async () => {
    await $('[data-testid="new-sql-tab"]').click()
    await $('[data-testid="tab-sql 1"]').waitForExist()
    await browser.keys([CTRL, 'w'])
    await $('[data-testid="tab-sql 1"]').waitForExist({ reverse: true })

    await (await visible('[data-testid="close-tab-app.orders"]')).click()
    await $('[data-testid="tab-app.orders"]').waitForExist({ reverse: true })
    // The neighbor takes over.
    await waitForText('[data-testid="table-title"]', 'app.customers')
  })

  it('runs sql in an editor tab', async () => {
    await typeSql('select count(*) as customer_count from app.customers')
    await clickVisible('[data-testid="run-query"]')

    await waitForText('[data-testid="sql-results"]', 'customer_count')
    await waitForText('[data-testid="sql-results"]', '3')
    await browser.saveScreenshot('./e2e/screenshots/workspace-sql.png')

    // Back to the table tab: grid state survived.
    await $('[data-testid="tab-app.customers"]').click()
    await waitForText('[data-testid="table-title"]', 'app.customers')
  })

  it('keeps separate content in a second editor tab', async () => {
    await $('[data-testid="new-sql-tab"]').click()
    await $('[data-testid="tab-sql 2"]').waitForExist()
    await typeSql('select 2 as second_editor')
    await waitForText('[data-testid="sql-input"]', 'second_editor')

    await $('[data-testid="tab-sql 1"]').click()
    await waitForText('[data-testid="sql-input"]', 'customer_count')
    await browser.keys([CTRL, 'w'])
    await $('[data-testid="tab-sql 1"]').waitForExist({ reverse: true })
  })

  it('shows a database error and keeps the session usable', async () => {
    await typeSql('select * from app.nope')
    await clickVisible('[data-testid="run-query"]')
    await waitForText('[data-testid="sql-error"]', 'does not exist')

    // The pinned session survives a failed statement.
    await typeSql('select 42 as answer')
    await clickVisible('[data-testid="run-query"]')
    await waitForText('[data-testid="sql-results"]', 'answer')
  })

  it('splits a multi-statement script into result tabs', async () => {
    await typeSql('select 1 as first; select 2 as second')
    await clickVisible('[data-testid="run-query"]')

    await waitForText('[data-testid="sql-results"]', 'first')
    const tabs = await $$('[data-testid="sql-results"] [role="tab"]')
    expect(tabs).toHaveLength(2)

    await tabs[1].click()
    await waitForText('[data-testid="sql-results"]', 'second')
  })

  it('reloads a statement from history', async () => {
    await clickVisible('[data-testid="query-history"]')
    await $('[data-testid="history-list"]').waitForExist()

    // Several statements ran before this: search narrows to the one we want.
    await $('[data-testid="history-search"]').setValue('customer_count')
    await waitForText('[data-testid="history-list"]', 'customer_count')

    await $('[data-testid="history-list"] [data-testid="history-item"]').click()
    await $('[data-testid="history-list"]').waitForExist({ reverse: true })
    await waitForText('[data-testid="sql-input"]', 'customer_count')
    await browser.saveScreenshot('./e2e/screenshots/workspace-sql-history.png')
  })

  it('edits a cell through the preview transaction', async () => {
    await $('[data-testid="tab-app.customers"]').click()
    await waitForText('[data-testid="table-title"]', 'app.customers')

    // Sorted name desc from the earlier test: first row is Grace Hopper.
    await beginCellEdit('[data-testid="grid-body"] tr:first-child td:nth-child(2)')
    await setEditorValue('[data-testid="cell-editor"]', 'Grace Renamed')
    await browser.keys(['Enter'])

    await waitForText('[data-testid="apply-changes"]', 'Apply (1)')
    await clickVisible('[data-testid="apply-changes"]')
    await waitForText('[data-testid="sql-preview"]', 'UPDATE "app"."customers" SET "name" = \'Grace Renamed\'')
    await $('[data-testid="confirm-apply"]').click()
    await waitForText('[data-testid="grid-body"] tr:first-child', 'Grace Renamed')
    await browser.saveScreenshot('./e2e/screenshots/workspace-edit.png')

    // Restore the fixture for the remaining specs.
    await beginCellEdit('[data-testid="grid-body"] tr:first-child td:nth-child(2)')
    await setEditorValue('[data-testid="cell-editor"]', 'Grace Hopper')
    await browser.keys(['Enter'])
    await clickVisible('[data-testid="apply-changes"]')
    await $('[data-testid="confirm-apply"]').click()
    await waitForText('[data-testid="grid-body"] tr:first-child', 'Grace Hopper')
    await waitForToastsGone()
  })

  it('discards a pending insert without applying', async () => {
    await clickVisible('[data-testid="add-row"]')
    await $('[data-testid="insert-row"]').waitForExist()
    await clickVisible('[data-testid="remove-insert"]')
    await $('[data-testid="insert-row"]').waitForExist({ reverse: true })
    await $('[data-testid="apply-changes"]').waitForExist({ reverse: true })
  })

  it('inserts a row, then deletes it, both through apply', async () => {
    await clickVisible('[data-testid="add-row"]')
    await setEditorValue('[data-testid="insert-name"]', 'Temp Person')
    await clickVisible('[data-testid="apply-changes"]')
    await waitForText('[data-testid="sql-preview"]', 'INSERT INTO "app"."customers" ("name")')
    await $('[data-testid="confirm-apply"]').click()
    // name desc: Temp Person sorts before Grace Hopper.
    await waitForText('[data-testid="grid-body"] tr:first-child', 'Temp Person')
    await waitForVisibleRows(4)
    await waitForToastsGone()

    await clickVisible('[data-testid="grid-body"] tr:first-child td:nth-child(2)')
    await clickVisible('[data-testid="delete-row"]')
    await clickVisible('[data-testid="apply-changes"]')
    await waitForText('[data-testid="sql-preview"]', 'DELETE FROM "app"."customers"')
    await $('[data-testid="confirm-apply"]').click()
    await waitForVisibleRows(3)
    await waitForToastsGone()
  })

  it('edits a pk-less table via ctid', async () => {
    await $('[data-testid="tree-filter"]').setValue('audit')
    await $('[data-testid="table-public.audit_log"]').click()
    await waitForText('[data-testid="table-title"]', 'public.audit_log')

    await (await visible('[data-testid="no-pk-banner"]')).waitForExist()
    await clickVisible('[data-testid="enable-ctid"]')
    await waitForVisibleRows(2)

    // Unique value: audit_log mutations persist across e2e runs (no reseed).
    const edited = `ctid edit ${Date.now()}`
    await beginCellEdit('[data-testid="grid-body"] tr:first-child td:nth-child(2)')
    await setEditorValue('[data-testid="cell-editor"]', edited)
    await browser.keys(['Enter'])
    await clickVisible('[data-testid="apply-changes"]')
    await waitForText('[data-testid="sql-preview"]', '"ctid" = ')
    await $('[data-testid="confirm-apply"]').click()
    await waitForText('[data-testid="grid-body"]', edited)
    await waitForToastsGone()
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
