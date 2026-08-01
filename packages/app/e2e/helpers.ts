import { $, $$, browser } from '@wdio/globals'
import { TEST_DBS } from './fixtures'

/// WebDriver unicode for the Control key (avoids importing webdriverio's Key enum).
export const CTRL = '\u{E009}'

export async function createConnection(name: string, kind: keyof typeof TEST_DBS) {
  const db = TEST_DBS[kind]
  await $('[data-testid="new-connection"]').click()
  await $('[data-testid="field-name"]').setValue(name)
  if (kind !== 'postgres') {
    await $('[data-testid="field-kind"]').click()
    await $(`[role="option"]*=${kind === 'mysql' ? 'MySQL' : kind}`).click()
  }
  await $('[data-testid="field-host"]').setValue(db.host)
  await $('[data-testid="field-port"]').setValue(db.port)
  await $('[data-testid="field-database"]').setValue(db.database)
  await $('[data-testid="field-user"]').setValue(db.user)
  await $('[data-testid="field-password"]').setValue(db.password)
  await $('[data-testid="save-connection"]').click()
  await $('[data-testid="connection-row"]').waitForExist()
  // The dialog overlay swallows clicks while its close animation runs.
  await $('[data-testid="field-name"]').waitForExist({ reverse: true })
}

export async function createPostgresConnection(name: string) {
  await createConnection(name, 'postgres')
}

export async function createSqliteConnection(name: string, path: string) {
  await $('[data-testid="new-connection"]').click()
  await $('[data-testid="field-name"]').setValue(name)
  await $('[data-testid="field-kind"]').click()
  await $('[role="option"]*=SQLite').click()
  await $('[data-testid="field-path"]').setValue(path)
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

/// First displayed match: inactive tabs keep their panels mounted but hidden,
/// so bare selectors can land on an invisible duplicate. Waits, because a
/// conditional field is mounted a frame after the click that reveals it.
export async function visible(selector: string, timeout = 5000) {
  let found: WebdriverIO.Element | undefined
  await browser.waitUntil(
    async () => {
      for (const element of await $$(selector)) {
        if (await element.isDisplayed()) {
          found = element
          return true
        }
      }
      return false
    },
    { timeout, timeoutMsg: `no visible element for ${selector}` },
  )
  return found!
}

export async function clickVisible(selector: string) {
  await (await visible(selector)).click()
}

/// Replaces the visible editor's content. One insertText event: per-key typing
/// drops keystrokes under WebKitWebDriver.
export async function typeSql(sql: string) {
  // Make some sql tab active: the current one, else the first, else a new one.
  const activeSql = $('[data-testid="tab-bar"] [data-testid^="tab-sql"][data-active]')
  if (!(await activeSql.isExisting())) {
    const anySql = $('[data-testid^="tab-sql"]')
    if (await anySql.isExisting())
      await anySql.click()
    else
      await $('[data-testid="new-sql-tab"]').click()
  }
  // The whole select-all + insert can race the editor grabbing focus: retry.
  for (let attempt = 0; ; attempt++) {
    await (await visible('[data-testid="sql-input"] .cm-content')).click()
    // Ctrl+A through CodeMirror's own keymap: execCommand('selectAll') can land
    // outside the editor and leave the previous statement in place.
    await browser.keys([CTRL, 'a'])
    await browser.execute(text => document.execCommand('insertText', false, text), sql)
    try {
      await browser.waitUntil(
        async () => (await visibleText('[data-testid="sql-input"] .cm-content')).trim() === sql,
        { timeout: 3_000 },
      )
      return
    }
    catch (error) {
      if (attempt >= 2)
        throw new Error(`the editor never held exactly "${sql}"`, { cause: error })
    }
  }
}

// textContent via execute: WebKitWebDriver's getText returns '' on truncated
// spans. Skips elements hidden by v-show (offsetParent is null).
async function visibleText(selector: string): Promise<string> {
  return await browser.execute((sel) => {
    for (const element of document.querySelectorAll(sel)) {
      if (element instanceof HTMLElement && element.offsetParent === null)
        continue
      return element.textContent ?? ''
    }
    return ''
  }, selector)
}

/// Single-click first: selecting the cell opens the inspector and shifts the
/// layout, which would swallow the second click of a direct double-click.
export async function beginCellEdit(selector: string) {
  await clickVisible(selector)
  await (await visible(selector)).doubleClick()
  await visible('[data-testid="cell-editor"]')
}

/// Focus a visible input and replace its content in one insertText event.
export async function setEditorValue(selector: string, value: string) {
  await (await visible(selector)).click()
  await browser.keys([CTRL, 'a'])
  await browser.execute(text => document.execCommand('insertText', false, text), value)
}

/// Toasts are position:fixed (offsetParent null), invisible to waitForText.
export async function waitForToast(text: string, timeout = 10_000) {
  await browser.waitUntil(
    async () =>
      browser.execute(
        needle => [...document.querySelectorAll('[data-sonner-toast]')]
          .some(toast => (toast.textContent ?? '').includes(needle)),
        text,
      ),
    { timeout, timeoutMsg: `no toast containing "${text}"` },
  )
}

/// Toasts overlay the grid footer and intercept clicks: wait them out.
export async function waitForToastsGone(timeout = 8_000) {
  await browser.waitUntil(
    async () => browser.execute(() => document.querySelector('[data-sonner-toast]') === null),
    { timeout, timeoutMsg: 'a toast never went away' },
  )
}

/// Rows of the visible grid only: hidden tabs keep their grids mounted.
export async function waitForVisibleRows(count: number, timeout = 10_000) {
  await browser.waitUntil(
    async () => {
      const rows = await browser.execute(() => {
        const body = [...document.querySelectorAll('[data-testid="grid-body"]')]
          .find(el => el instanceof HTMLElement && el.offsetParent !== null)
        return body ? body.querySelectorAll('tr').length : -1
      })
      return rows === count
    },
    { timeout, timeoutMsg: `the visible grid never held ${count} rows` },
  )
}

export async function waitForText(selector: string, text: string, timeout = 10_000) {
  // Collapsed: markup indentation lands in textContent, so a phrase spread
  // over several elements would never match as typed.
  const collapse = (value: string) => value.replace(/\s+/g, ' ').trim()
  await browser.waitUntil(
    async () => collapse(await visibleText(selector)).includes(collapse(text)),
    {
      timeout,
      timeoutMsg: `${selector} never contained "${text}"`,
    },
  )
}
