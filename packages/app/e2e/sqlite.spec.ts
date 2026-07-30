import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { $, browser } from '@wdio/globals'
import { clickVisible, createSqliteConnection, deleteFirstConnection, typeSql, waitForText } from './helpers'

// A zero-byte file is a valid empty sqlite database: no seed tooling needed.
const DB_PATH = path.join(os.tmpdir(), 'soquel-e2e.sqlite')

describe('sqlite workspace', () => {
  before(() => {
    fs.rmSync(DB_PATH, { force: true })
    fs.rmSync(`${DB_PATH}-wal`, { force: true })
    fs.rmSync(`${DB_PATH}-shm`, { force: true })
    fs.writeFileSync(DB_PATH, '')
  })

  after(() => {
    fs.rmSync(DB_PATH, { force: true })
    fs.rmSync(`${DB_PATH}-wal`, { force: true })
    fs.rmSync(`${DB_PATH}-shm`, { force: true })
  })

  it('creates a file-backed connection and opens its workspace', async () => {
    await $('[data-testid="empty-state"]').waitForExist()
    await createSqliteConnection('fixture sqlite', DB_PATH)

    await $('[data-testid="open-fixture sqlite"]').click()
    await waitForText('[data-testid="workspace-name"]', 'fixture sqlite')
    await waitForText('[data-testid="server-version"]', 'SQLite')
    await browser.saveScreenshot('./e2e/screenshots/sqlite-workspace.png')
  })

  it('creates tables through sql and finds them after a schema refresh', async () => {
    await typeSql(
      'create table todos (id integer primary key, title text not null, done integer default 0);'
      + ' insert into todos (title, done) values (\'write the sqlite driver\', 1), (\'ship it\', 0);'
      + ' create table logs (message text);',
    )
    await clickVisible('[data-testid="run-query"]')
    // create + 2 inserts + create: the footer sums affected rows.
    await waitForText('[data-testid="query-timing"]', '2 rows')

    await clickVisible('[data-testid="refresh-schema"]')
    await $('[data-testid="table-main.todos"]').waitForExist({ timeout: 10_000 })
  })

  it('browses the table and offers the rowid rescue on pk-less tables', async () => {
    await $('[data-testid="table-main.todos"]').click()
    await waitForText('[data-testid="table-title"]', 'main.todos')
    await waitForText('[data-testid="grid-body"]', 'write the sqlite driver')
    await browser.saveScreenshot('./e2e/screenshots/sqlite-grid.png')

    await $('[data-testid="table-main.logs"]').click()
    await waitForText('[data-testid="table-title"]', 'main.logs')
    await waitForText('[data-testid="no-pk-banner"]', 'edit via rowid')
  })

  it('renders the explain query plan tree', async () => {
    await typeSql('select * from todos where id = 1')
    await clickVisible('[data-testid="explain-menu"]')
    await $('[data-testid="explain-plain"]').click()
    await waitForText('[data-testid="explain-tree"]', 'Index search')
    await waitForText('[data-testid="explain-tree"]', 'query structure only')
    await browser.saveScreenshot('./e2e/screenshots/sqlite-explain.png')
  })

  it('disconnects and cleans up', async () => {
    await $('[data-testid="workspace-disconnect"]').click()
    await $('[data-testid="connection-row"]').waitForExist()
    await deleteFirstConnection()
  })
})
