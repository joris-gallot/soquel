import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { $, $$, browser } from '@wdio/globals'
import { createPostgresConnection, exportConnectionsTo, openImportDialog, setEditorValue, waitForText, waitForToast } from './helpers'

const PLAIN = path.join(os.tmpdir(), 'soquel-e2e-plain.json')
const SEALED = path.join(os.tmpdir(), 'soquel-e2e-sealed.json')
const BROKEN = path.join(os.tmpdir(), 'soquel-e2e-broken.json')
const FOREIGN = path.join(os.tmpdir(), 'soquel-e2e-foreign.json')

describe('connection transfer', () => {
  after(() => {
    for (const file of [PLAIN, SEALED, BROKEN, FOREIGN])
      fs.rmSync(file, { force: true })
  })

  it('exports the connections it was given', async () => {
    await $('[data-testid="empty-state"]').waitForExist()
    await createPostgresConnection('exported pg')

    await exportConnectionsTo(PLAIN)
    await exportConnectionsTo(SEALED, 'correct horse battery')

    // Shareable by default: no secret field at all, so nothing to leak.
    const plain = JSON.parse(fs.readFileSync(PLAIN, 'utf8'))
    expect(plain.document.connections[0].name).toBe('exported pg')
    expect(plain.document.connections[0].secret).toBeUndefined()
    // Sealed: the payload is ciphertext, names included.
    const sealed = JSON.parse(fs.readFileSync(SEALED, 'utf8'))
    expect(sealed.document).toBeUndefined()
    expect(sealed.payload).toEqual(expect.any(String))
    expect(fs.readFileSync(SEALED, 'utf8')).not.toContain('exported pg')
  })

  it('asks before writing an export it cannot re-read', async () => {
    await $('[data-testid="connections-menu"]').click()
    await $('[data-testid="open-export"]').waitForDisplayed()
    await $('[data-testid="open-export"]').click()
    await $('[data-testid="export-include-secrets"]').waitForDisplayed()
    await $('[data-testid="export-include-secrets"]').click()
    // insertText, not per-key typing: WebKitWebDriver drops keystrokes and a
    // half-typed pair reads as "too short" instead of "does not match".
    await setEditorValue('[data-testid="export-passphrase"]', 'correct horse')
    await setEditorValue('[data-testid="export-passphrase-confirm"]', 'correct hose')

    // Stops here: past this point the native picker takes over.
    await $('[data-testid="run-export"]').click()
    await waitForText('[data-testid="export-error"]', 'do not match')
    await browser.keys(['Escape'])
    await $('[data-testid="export-passphrase"]').waitForExist({ reverse: true })
  })

  it('previews a plain file, then imports it as a duplicate', async () => {
    await openImportDialog(PLAIN)
    await waitForText('[data-testid="import-counts"]', '1 connections, 0 tunnels')
    // The connection it was exported from is still here: it reads as existing.
    await $('[data-testid="import-duplicate"]').waitForExist()
    await $('[data-testid="strategy-keep-both"]').click()

    await $('[data-testid="run-import"]').click()
    await waitForToast('1 added')
    expect(await $$('[data-testid="connection-row"]').length).toBe(2)
  })

  it('skips a duplicate without touching what is here', async () => {
    await openImportDialog(PLAIN)
    await $('[data-testid="strategy-skip"]').click()
    await $('[data-testid="run-import"]').click()
    await waitForToast('1 skipped')
    expect(await $$('[data-testid="connection-row"]').length).toBe(2)
  })

  it('keeps the passphrase field on screen until the file opens', async () => {
    await openImportDialog(SEALED)
    await $('[data-testid="import-passphrase"]').waitForExist()
    // Nothing is readable yet: no entry list behind the lock.
    expect(await $('[data-testid="import-counts"]').isExisting()).toBe(false)

    await $('[data-testid="import-passphrase"]').setValue('wrong one')
    await $('button=Unlock').click()
    await waitForText('[data-testid="import-error"]', 'wrong passphrase')
    await $('[data-testid="import-passphrase"]').waitForExist()

    await $('[data-testid="import-passphrase"]').setValue('correct horse battery')
    await $('button=Unlock').click()
    await waitForText('[data-testid="import-counts"]', '1 connections')
    // The sealed file carries the password the plain one left out.
    await $('[data-testid="import-entry"]').waitForExist()
    await browser.keys(['Escape'])
  })

  it('refuses a file that is not an export at all', async () => {
    fs.writeFileSync(FOREIGN, JSON.stringify({ connections: [] }))
    await openImportDialog(FOREIGN)
    await waitForText('[data-testid="import-error"]', 'not a soquel connections file')
    await browser.keys(['Escape'])
  })

  it('blocks the whole import on one invalid entry', async () => {
    fs.writeFileSync(BROKEN, JSON.stringify({
      soquel: 'soquel-connections',
      version: 1,
      document: {
        connections: [
          { name: 'fine', env: 'dev', params: { kind: 'postgres', host: 'db', port: 5432, database: 'app', user: 'soquel' } },
          { name: 'no host', env: 'dev', params: { kind: 'postgres', host: '', port: 5432, database: 'app', user: 'soquel' } },
        ],
        tunnels: [],
      },
    }))

    await openImportDialog(BROKEN)
    // The valid entry rides along: one bad line is enough to hold the batch.
    await waitForText('[data-testid="import-problem"]', 'the host is empty')
    expect(await $('[data-testid="run-import"]').isEnabled()).toBe(false)
    await browser.keys(['Escape'])
  })
})
