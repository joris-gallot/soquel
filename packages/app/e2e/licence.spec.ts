import { $, browser } from '@wdio/globals'
import { waitForText } from './helpers'

/// The endpoint is a closed port (see wdio.conf.ts), so this covers the whole chain
/// down to the reason and its wording. The happy path needs the real signing key and
/// stays a manual check against a local service.
describe('the licence dialog', () => {
  before(async () => {
    await $('[data-testid="open-palette"]').click()
    await $('[data-testid="palette-licence"]').click()
    await $('[data-testid="licence-key"]').waitForDisplayed()
    await browser.saveScreenshot('./e2e/screenshots/licence-dialog.png')
  })

  it('blames the connection, not the key, when the server never answers', async () => {
    await $('[data-testid="licence-key"]').setValue('SOQUEL-0000-0000-0000')
    await $('[data-testid="apply-licence"]').click()

    // Telling someone their key is invalid because a request failed is the worst
    // message this dialog could show.
    await waitForText('[data-testid="licence-outcome"]', 'No answer from the licence server')
  })

  it('keeps the file path working behind its own toggle', async () => {
    await $('[data-testid="licence-file-toggle"]').click()
    await $('[data-testid="licence-token"]').waitForDisplayed()
    await $('[data-testid="licence-token"]').setValue('not-a-licence')
    await $('[data-testid="apply-licence-file"]').click()

    await waitForText('[data-testid="licence-outcome"]', 'does not look like a licence')
  })
})
