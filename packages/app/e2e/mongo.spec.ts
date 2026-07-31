import { $, browser } from '@wdio/globals'
import { TEST_MONGO } from './fixtures'
import { deleteFirstConnection, setEditorValue, waitForText } from './helpers'

async function runFilter(filter: string) {
  await $('[data-testid="doc-filter"]').setValue(filter)
  await browser.keys('Enter')
}

async function runConsole(source: string) {
  await $('[data-testid="console-input"]').setValue(source)
  await browser.keys('Enter')
}

describe('mongo workspace', () => {
  it('creates a mongo connection and opens the document browser', async () => {
    await $('[data-testid="empty-state"]').waitForExist()
    await $('[data-testid="new-connection"]').click()
    await $('[data-testid="field-name"]').setValue('fixture mongo')
    await $('[data-testid="field-kind"]').click()
    await $('[role="option"]*=MongoDB').click()
    await $('[data-testid="field-host"]').setValue(TEST_MONGO.host)
    await $('[data-testid="field-port"]').setValue(TEST_MONGO.port)
    await $('[data-testid="field-user"]').setValue(TEST_MONGO.user)
    await $('[data-testid="field-password"]').setValue(TEST_MONGO.password)
    await $('[data-testid="save-connection"]').click()
    await $('[data-testid="connection-row"]').waitForExist()
    await $('[data-testid="field-name"]').waitForExist({ reverse: true })

    await $('[data-testid="open-fixture mongo"]').click()
    await waitForText('[data-testid="workspace-name"]', 'fixture mongo')
    await waitForText('[data-testid="server-version"]', 'Mongo')
    // The doc surface replaces the schema tree: no sql tabs, a collection list.
    await $('[data-testid="collection-filter"]').waitForExist()
    await $('[data-testid="new-sql-tab"]').waitForExist({ reverse: true })
    await browser.saveScreenshot('./e2e/screenshots/mongo-workspace.png')
  })

  it('switches to the seeded database and filters documents', async () => {
    await $('[data-testid="doc-db"]').click()
    await $('[data-testid="doc-db-option-soquel_e2e"]').click()
    await waitForText('[data-testid="doc-db"]', 'soquel_e2e')

    await $('[data-testid="collection-users"]').waitForExist()
    await $('[data-testid="collection-users"]').click()
    await $('[data-testid="doc-row-0"]').waitForExist()
    // 200 seeded users, one page of 100: more to load.
    await $('[data-testid="doc-more"]').waitForExist()

    // Half the seed is plan=pro; a filtered count is exact.
    await runFilter('{ "plan": "pro" }')
    await waitForText('[data-testid="doc-count"]', '100 docs')
  })

  it('opens a document and edits it through the canonical editor', async () => {
    await runFilter('{ "email": "user0@example.com" }')
    await waitForText('[data-testid="doc-count"]', '1 doc')
    await $('[data-testid="doc-row-0"]').click()
    await waitForText('[data-testid="doc-json"]', 'user0@example.com')

    await $('[data-testid="edit-doc"]').click()
    const draft = await $('[data-testid="doc-editor"]').getValue()
    // Idempotent across reruns: the plan is already "enterprise" the second time.
    await setEditorValue('[data-testid="doc-editor"]', draft.replace('"free"', '"enterprise"'))
    await $('[data-testid="save-doc"]').click()
    await waitForText('[data-testid="doc-json"]', 'enterprise')
    await browser.saveScreenshot('./e2e/screenshots/mongo-document.png')
  })

  it('deletes a disposable document with the two-step confirm', async () => {
    await $('[data-testid="collection-disposable"]').click()
    await $('[data-testid="doc-row-0"]').waitForExist()
    await $('[data-testid="doc-row-0"]').click()
    await waitForText('[data-testid="detail-id"]', 'delete-me-')
    const victim = await $('[data-testid="detail-id"]').getText()

    await $('[data-testid="delete-doc"]').click()
    await $('[data-testid="delete-doc"]').click()
    await waitForText('[data-testid="doc-detail"]', 'select a document')

    await runFilter(`{ "_id": "${victim.trim()}" }`)
    await waitForText('[data-testid="doc-count"]', '0 docs')
  })

  it('lists indexes and runs console queries', async () => {
    await $('[data-testid="collection-users"]').click()
    await $('[data-testid="doc-view-indexes"]').click()
    await $('td*=email_1').waitForExist()
    await $('[data-testid="index-unique"]').waitForExist()

    await $('[data-testid="doc-view-console"]').click()
    await runConsole('{ "plan": "pro" }')
    await waitForText('[data-testid="mongo-console"]', 'docs ·')
    await runConsole('[{ "$group": { "_id": "$plan", "n": { "$sum": 1 } } }]')
    await waitForText('[data-testid="mongo-console"]', '"n"')
    // Write stages stay blocked; the refusal lands in the console, not a toast.
    await runConsole('[{ "$out": "evil" }]')
    await waitForText('[data-testid="mongo-console"]', '$out writes to a collection')
  })

  it('cleans up and disconnects', async () => {
    await $('[data-testid="workspace-disconnect"]').click()
    await $('[data-testid="connection-row"]').waitForExist()
    await deleteFirstConnection()
  })
})
