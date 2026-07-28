import { $, browser } from '@wdio/globals'

describe('app boot', () => {
  it('renders the prompt and reaches the rust core', async () => {
    const status = $('[data-testid="core-status"]')
    await browser.waitUntil(
      async () => (await status.getText()).includes('core pong'),
      {
        timeout: 10_000,
        timeoutMsg: 'core never answered pong over IPC',
      },
    )
    await browser.saveScreenshot('./e2e/screenshots/home.png')
  })
})
