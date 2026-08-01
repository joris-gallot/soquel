import { $, browser } from '@wdio/globals'
import { TEST_DBS } from './fixtures'
import { waitForText } from './helpers'

const PG = TEST_DBS.postgres

describe('credential modes', () => {
  it('creates a connection that asks for its password', async () => {
    await $('[data-testid="new-connection"]').click()
    await $('[data-testid="field-name"]').setValue('ask me')
    await $('[data-testid="field-host"]').setValue(PG.host)
    await $('[data-testid="field-port"]').setValue(PG.port)
    await $('[data-testid="field-database"]').setValue(PG.database)
    await $('[data-testid="field-user"]').setValue(PG.user)

    await $('[data-testid="field-credential-mode"]').click()
    await $('[data-testid="credential-mode-prompt"]').click()
    // The password field stays, for the Test button only.
    await $('[data-testid="field-password"]').setValue(PG.password)
    await $('[data-testid="test-connection"]').click()
    await waitForText('[data-testid="test-result"]', 'Connection OK')
    await browser.saveScreenshot('./e2e/screenshots/credential-mode-prompt.png')

    await $('[data-testid="save-connection"]').click()
    await $('[data-testid="connection-row"]').waitForExist()
  })

  it('prompts at connect time, then opens the workspace', async () => {
    await $('[data-testid="toggle-connection"]').click()
    await $('[data-testid="secret-prompt-dialog"]').waitForDisplayed()
    await browser.saveScreenshot('./e2e/screenshots/credential-prompt-dialog.png')

    await $('[data-testid="field-prompt-secret"]').setValue(PG.password)
    await $('[data-testid="submit-prompt-secret"]').click()
    await waitForText('[data-testid="workspace-name"]', 'ask me')
  })

  it('shows the argv a credential command would run', async () => {
    await $('[data-testid="workspace-back"]').click()
    await $('[data-testid="row-menu"]').click()
    await $('[data-testid="row-edit"]').click()

    await $('[data-testid="field-credential-mode"]').click()
    await $('[data-testid="credential-mode-command"]').click()
    await $('[data-testid="field-credential-command"]').setValue('aws rds generate-db-auth-token --hostname {host}')
    await waitForText('[data-testid="credential-command-argv"]', 'generate-db-auth-token')

    // A pipeline has no shell to run it: the form says so before saving.
    await $('[data-testid="field-credential-command"]').setValue('vault read token | jq -r .value')
    await waitForText('[data-testid="credential-command-problem"]', 'needs a shell')
    await browser.saveScreenshot('./e2e/screenshots/credential-mode-command.png')

    await browser.keys(['Escape'])
    await $('[data-testid="field-name"]').waitForExist({ reverse: true })
  })
})
