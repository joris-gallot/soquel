import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { $, browser } from '@wdio/globals'
import { TEST_DBS } from './fixtures'
import { openImportDialog, waitForText } from './helpers'

const PG = TEST_DBS.postgres
const FILE = path.join(os.tmpdir(), 'soquel-e2e-import.json')

/// A file someone else wrote: its connection pays for its password by running
/// a local program.
function writeImportFile() {
  fs.writeFileSync(FILE, JSON.stringify({
    soquel: 'soquel-connections',
    version: 1,
    document: {
      connections: [{
        name: 'imported iam',
        env: 'dev',
        credential: { mode: 'command', command: `printf %s ${PG.password}` },
        params: {
          kind: 'postgres',
          host: PG.host,
          port: Number(PG.port),
          database: PG.database,
          user: PG.user,
        },
      }],
      tunnels: [],
    },
  }))
}

describe('imported credential command', () => {
  before(() => {
    writeImportFile()
  })

  after(() => {
    fs.rmSync(FILE, { force: true })
  })

  it('warns in the preview that the file carries a command', async () => {
    await $('[data-testid="empty-state"]').waitForExist()
    await openImportDialog(FILE)
    await waitForText('[data-testid="import-commands"]', '1 run a command')
    await $('[data-testid="import-entry-command"]').waitForExist()
    await browser.saveScreenshot('./e2e/screenshots/import-command-badge.png')

    await $('[data-testid="run-import"]').click()
    await $('[data-testid="connection-row"]').waitForExist()
  })

  it('does not run before the argv has been read and approved', async () => {
    await $('[data-testid="toggle-connection"]').click()
    await $('[data-testid="command-approval-dialog"]').waitForDisplayed()
    // One chip per argument: the split is part of what the user approves.
    await waitForText('[data-testid="command-approval-argv"]', 'printf')
    await waitForText('[data-testid="command-approval-argv"]', PG.password)
    await browser.saveScreenshot('./e2e/screenshots/command-approval.png')
  })

  it('runs it and connects once approved', async () => {
    await $('[data-testid="approve-command"]').click()
    await waitForText('[data-testid="workspace-name"]', 'imported iam')
  })

  it('asks again after the approval is revoked', async () => {
    await $('[data-testid="workspace-back"]').click()
    await $('[data-testid="toggle-connection"]').click()
    await $('[data-testid="status-disconnected"]').waitForExist()

    await $('[data-testid="row-menu"]').click()
    await $('[data-testid="row-revoke-command"]').click()

    await $('[data-testid="toggle-connection"]').click()
    await $('[data-testid="command-approval-dialog"]').waitForDisplayed()
  })
})
