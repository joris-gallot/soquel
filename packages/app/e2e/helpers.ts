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
