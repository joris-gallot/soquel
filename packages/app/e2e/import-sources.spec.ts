import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { $, browser } from '@wdio/globals'
import { TEST_DBS } from './fixtures'
import { waitForText, waitForToast } from './helpers'

const PG = TEST_DBS.postgres
// Mirrors SOQUEL_IMPORT_HOME in wdio.conf.ts: what the app reads instead of ~.
const IMPORT_HOME = path.join(os.tmpdir(), 'soquel-e2e-import-home')

describe('import from what the machine already has', () => {
  before(() => {
    fs.mkdirSync(IMPORT_HOME, { recursive: true })
    fs.writeFileSync(
      path.join(IMPORT_HOME, '.pgpass'),
      [
        '# a comment nobody imports',
        `${PG.host}:${PG.port}:${PG.database}:${PG.user}:${PG.password}`,
        '*:*:*:postgres:everywhere',
        '',
      ].join('\n'),
    )
  })

  after(() => {
    fs.rmSync(path.join(IMPORT_HOME, '.pgpass'), { force: true })
  })

  it('lists the sources it found, and what each holds', async () => {
    await $('[data-testid="empty-state"]').waitForExist()
    await $('[data-testid="connections-menu"]').click()
    await $('[data-testid="open-import"]').waitForDisplayed()
    await $('[data-testid="open-import"]').click()

    await $('[data-testid="import-sources-dialog"]').waitForDisplayed()
    await waitForText('[data-testid="import-source-pgpass"]', '2 entries found')
    // No service file was written: the row says so instead of vanishing.
    await waitForText('[data-testid="import-source-pg-service"]', 'not found')
    await browser.saveScreenshot('./e2e/screenshots/import-sources.png')
  })

  it('previews the concrete line and refuses the wildcard one', async () => {
    await $('[data-testid="import-source-pgpass"]').click()
    await waitForText('[data-testid="import-counts"]', '2 connections')
    await waitForText('[data-testid="import-problem"]', 'the host is a wildcard')
    // One bad line holds the batch: nothing half-imports.
    expect(await $('[data-testid="run-import"]').isEnabled()).toBe(false)
    await browser.saveScreenshot('./e2e/screenshots/import-pgpass-preview.png')
    await browser.keys(['Escape'])
    await $('[data-testid="import-counts"]').waitForExist({ reverse: true })
  })

  it('imports the file once the wildcard line is gone, without its password', async () => {
    fs.writeFileSync(
      path.join(IMPORT_HOME, '.pgpass'),
      `${PG.host}:${PG.port}:${PG.database}:${PG.user}:${PG.password}\n`,
    )
    await $('[data-testid="connections-menu"]').click()
    await $('[data-testid="open-import"]').click()
    await $('[data-testid="import-source-pgpass"]').click()
    await waitForText('[data-testid="import-counts"]', '1 connections')

    // The switch is off: the password stays in the file it came from.
    await $('[data-testid="import-with-secrets"]').waitForDisplayed()
    await $('[data-testid="run-import"]').click()
    await waitForToast('1 added')
    await waitForText('[data-testid="connection-row"]', `${PG.user}@${PG.host}:${PG.port}/${PG.database}`)
  })

  it('asks for the password it was not given', async () => {
    await $('[data-testid="toggle-connection"]').click()
    // No password imported, so postgres refuses and the error says why.
    await waitForToast('password')
  })
})
