import { $, browser } from '@wdio/globals'
import { TEST_DBS } from './fixtures'

export async function createPostgresConnection(name: string) {
  const pg = TEST_DBS.postgres
  await $('[data-testid="new-connection"]').click()
  await $('[data-testid="field-name"]').setValue(name)
  await $('[data-testid="field-host"]').setValue(pg.host)
  await $('[data-testid="field-port"]').setValue(pg.port)
  await $('[data-testid="field-database"]').setValue(pg.database)
  await $('[data-testid="field-user"]').setValue(pg.user)
  await $('[data-testid="field-password"]').setValue(pg.password)
  await $('[data-testid="save-connection"]').click()
  await $('[data-testid="connection-row"]').waitForExist()
  // The dialog overlay swallows clicks while its close animation runs.
  await $('[data-testid="field-name"]').waitForExist({ reverse: true })
}

export async function deleteFirstConnection() {
  await $('[data-testid="row-menu"]').click()
  await $('[data-testid="row-delete"]').click()
  await $('[data-testid="empty-state"]').waitForExist()
}

/// Replaces the editor content. One insertText event: per-key typing drops
/// keystrokes under WebKitWebDriver.
export async function typeSql(sql: string) {
  // The panel is hidden while the data view is up, and a hidden CM6 is not clickable.
  await $('[data-testid="view-sql"]').click()
  await $('[data-testid="sql-input"] .cm-content').click()
  // Ctrl+A through CodeMirror's own keymap: execCommand('selectAll') can land
  // outside the editor and leave the previous statement in place.
  await browser.keys(['', 'a'])
  await browser.execute(text => document.execCommand('insertText', false, text), sql)
  await browser.waitUntil(
    async () => {
      const content = await browser.execute(
        () => document.querySelector('[data-testid="sql-input"] .cm-content')?.textContent ?? '',
      )
      return content.trim() === sql
    },
    { timeout: 10_000, timeoutMsg: `the editor never held exactly "${sql}"` },
  )
}

export async function waitForText(selector: string, text: string, timeout = 10_000) {
  // textContent via execute: WebKitWebDriver's getText returns '' on truncated spans.
  await browser.waitUntil(
    async () => {
      const value = await browser.execute(
        sel => document.querySelector(sel)?.textContent ?? '',
        selector,
      )
      return value.includes(text)
    },
    {
      timeout,
      timeoutMsg: `${selector} never contained "${text}"`,
    },
  )
}
