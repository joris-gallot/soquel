import { $, browser } from '@wdio/globals'
import { TEST_REDIS } from './fixtures'
import { deleteFirstConnection, waitForText } from './helpers'

async function runConsole(command: string) {
  await $('[data-testid="console-input"]').setValue(command)
  await browser.keys('Enter')
}

describe('redis workspace', () => {
  it('creates a redis connection and opens the key browser', async () => {
    await $('[data-testid="empty-state"]').waitForExist()
    await $('[data-testid="new-connection"]').click()
    await $('[data-testid="field-name"]').setValue('fixture redis')
    await $('[data-testid="field-kind"]').click()
    await $('[role="option"]*=Redis').click()
    await $('[data-testid="field-host"]').setValue(TEST_REDIS.host)
    await $('[data-testid="field-port"]').setValue(TEST_REDIS.port)
    await $('[data-testid="field-password"]').setValue(TEST_REDIS.password)
    await $('[data-testid="save-connection"]').click()
    await $('[data-testid="connection-row"]').waitForExist()
    await $('[data-testid="field-name"]').waitForExist({ reverse: true })

    await $('[data-testid="open-fixture redis"]').click()
    await waitForText('[data-testid="workspace-name"]', 'fixture redis')
    await waitForText('[data-testid="server-version"]', 'Redis')
    // The kv surface replaces the schema tree: no sql tabs, a key pattern input.
    await $('[data-testid="key-pattern"]').waitForExist()
    await $('[data-testid="new-sql-tab"]').waitForExist({ reverse: true })
    await browser.saveScreenshot('./e2e/screenshots/redis-workspace.png')
  })

  it('seeds keys through the console and scans them', async () => {
    await $('[data-testid="kv-view-console"]').click()
    // Self-clean: a previously aborted run leaves e2e:* keys behind.
    await runConsole('DEL e2e:greeting e2e:list')
    await waitForText('[data-testid="redis-console"]', '(integer)')
    await runConsole('SET e2e:greeting "hello soquel"')
    await waitForText('[data-testid="redis-console"]', 'OK')
    await runConsole('RPUSH e2e:list one two three')
    await waitForText('[data-testid="redis-console"]', '(integer) 3')
    await runConsole('EXPIRE e2e:greeting 7200')

    // Default search is contains: no glob syntax needed.
    await $('[data-testid="key-pattern"]').setValue('e2e:')
    await browser.keys('Enter')
    await $('[data-testid="key-e2e:greeting"]').waitForExist()
    await $('[data-testid="key-e2e:list"]').waitForExist()
    await waitForText('[data-testid="key-count"]', '2 keys')

    // Glob mode passes the raw MATCH pattern through.
    await $('[data-testid="key-glob"]').click()
    await $('[data-testid="key-pattern"]').setValue('e2e:gr*')
    await browser.keys('Enter')
    await $('[data-testid="key-e2e:greeting"]').waitForExist()
    await waitForText('[data-testid="key-count"]', '1 key')
    await $('[data-testid="key-glob"]').click()
    await $('[data-testid="key-pattern"]').setValue('e2e:')
    await browser.keys('Enter')
    await waitForText('[data-testid="key-count"]', '2 keys')
  })

  it('reads and edits a string key with its ttl', async () => {
    await $('[data-testid="key-e2e:greeting"]').click()
    await waitForText('[data-testid="detail-key"]', 'e2e:greeting')
    await waitForText('[data-testid="detail-ttl"]', 'ttl 2h')
    const editor = $('[data-testid="string-editor"]')
    await editor.waitForExist()
    await editor.setValue('updated value')
    await $('[data-testid="save-string"]').click()
    await waitForText('[data-testid="detail-ttl"]', 'ttl', 5_000)
    // KEEPTTL: the edit must not clear the expiry.
    await waitForText('[data-testid="detail-ttl"]', 'ttl 2h')
    await browser.saveScreenshot('./e2e/screenshots/redis-key.png')
  })

  it('switches databases from the workspace selector', async () => {
    // db 1 is empty: the browser resets and the count drops to zero.
    await $('[data-testid="kv-db"]').click()
    await $('[data-testid="kv-db-option-1"]').click()
    await waitForText('[data-testid="kv-db"]', 'db 1')
    await waitForText('[data-testid="key-count"]', '0 keys')

    await $('[data-testid="kv-db"]').click()
    await $('[data-testid="kv-db-option-0"]').click()
    await waitForText('[data-testid="kv-db"]', 'db 0')
    await $('[data-testid="key-e2e:greeting"]').waitForExist()
  })

  it('browses a list key and deletes it', async () => {
    await $('[data-testid="key-e2e:list"]').click()
    await waitForText('[data-testid="detail-key"]', 'e2e:list')
    await waitForText('[data-testid="key-value"]', 'two')

    // Two-step confirm, then the list refreshes without the key.
    await $('[data-testid="delete-key"]').click()
    await $('[data-testid="delete-key"]').click()
    await $('[data-testid="key-e2e:list"]').waitForExist({ reverse: true })
  })

  it('cleans up and disconnects', async () => {
    await $('[data-testid="kv-view-console"]').click()
    await runConsole('DEL e2e:greeting')
    await waitForText('[data-testid="redis-console"]', '(integer)')
    await $('[data-testid="workspace-disconnect"]').click()
    await $('[data-testid="connection-row"]').waitForExist()
    await deleteFirstConnection()
  })
})
