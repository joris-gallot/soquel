import { $, browser } from '@wdio/globals'

describe('app boot', () => {
  it('renders the prompt and reaches the rust core', async () => {
    const version = $('[data-testid="app-version"]')
    // The version arrives over IPC: rendering it proves the core answered.
    await browser.waitUntil(
      async () => /^soquel \d+\.\d+\.\d+$/.test(await version.getText()),
      {
        timeout: 10_000,
        timeoutMsg: 'the core never reported its version over IPC',
      },
    )
    await browser.saveScreenshot('./e2e/screenshots/home.png')
  })
})
