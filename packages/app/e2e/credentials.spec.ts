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

  // No sshd here: the form's own wiring, not a tunnel bring-up.
  it('offers the same modes on a tunnel, and only where a secret is ours', async () => {
    // The previous test left us in the workspace; tunnels live on the list.
    await $('[data-testid="workspace-back"]').click()
    await $('[data-testid="new-tunnel"]').click()
    await $('[data-testid="field-tunnel-name"]').waitForExist()
    // Agent auth: the key never leaves the agent, so there is nothing to source.
    await $('[data-testid="field-tunnel-credential-mode"]').waitForExist({ reverse: true })

    await $('[data-testid="field-tunnel-auth"]').click()
    await $('[role="option"]*=Key file').click()
    await $('[data-testid="field-tunnel-credential-mode"]').click()
    await $('[data-testid="tunnel-credential-mode-command"]').click()
    await $('[data-testid="field-tunnel-credential-command"]').setValue('vault-ssh-password --host {host}')
    // The placeholder reaches the core intact, braces included.
    await waitForText('[data-testid="tunnel-credential-command-argv"]', '{host}')
    await browser.saveScreenshot('./e2e/screenshots/credential-tunnel-command.png')

    await browser.keys(['Escape'])
    await $('[data-testid="field-tunnel-name"]').waitForExist({ reverse: true })
  })

  it('shows the argv a credential command would run', async () => {
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
