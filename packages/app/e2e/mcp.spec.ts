import { $, browser, expect } from '@wdio/globals'
import { createSqliteConnection, deleteFirstConnection } from './helpers'

const ENDPOINT = 'http://127.0.0.1:52700/mcp'

describe('mcp server', () => {
  it('shows the panel, off by default', async () => {
    await $('[data-testid="mcp-stopped"]').waitForExist()
    await $('[data-testid="mcp-regenerate"]').waitForExist()
  })

  it('starts the real server; requests without the token get 401', async () => {
    await $('[data-testid="mcp-toggle"]').click()
    await $('[data-testid="mcp-running"]').waitForExist()
    await $('[data-testid="mcp-details"]').waitForExist()
    await browser.saveScreenshot('./e2e/screenshots/mcp-panel.png')

    const response = await fetch(ENDPOINT, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Accept': 'application/json, text/event-stream' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {} }),
    })
    expect(response.status).toBe(401)
  })

  it('stops the server and frees the port', async () => {
    await $('[data-testid="mcp-toggle"]').click()
    await $('[data-testid="mcp-stopped"]').waitForExist()

    await browser.waitUntil(async () => {
      try {
        await fetch(ENDPOINT, { method: 'POST' })
        return false
      }
      catch {
        return true
      }
    }, { timeout: 5000, timeoutMsg: 'mcp port still answers after stop' })
  })

  it('badges connections opted in for agents', async () => {
    await createSqliteConnection('agent sqlite', '/tmp/soquel-e2e-agent.db')
    await $('[data-testid="connection-row"]').waitForExist()
    await expect($('[data-testid="agent-badge"]')).not.toExist()

    await $('[data-testid="row-menu"]').click()
    await $('[data-testid="row-edit"]').click()
    await $('[data-testid="field-agent-access"]').click()
    await $('[data-testid="agent-access-read-only"]').click()
    await $('[data-testid="save-connection"]').click()
    await $('[data-testid="field-agent-access"]').waitForExist({ reverse: true })

    await $('[data-testid="agent-badge"]').waitForExist()
    await browser.saveScreenshot('./e2e/screenshots/mcp-agent-badge.png')
  })

  it('deletes the connection', async () => {
    await deleteFirstConnection()
  })
})
