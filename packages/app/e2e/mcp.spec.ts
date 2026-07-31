import { $, browser, expect } from '@wdio/globals'
import { createSqliteConnection, deleteFirstConnection } from './helpers'

const ENDPOINT = 'http://127.0.0.1:52700/mcp'

/// Minimal streamable-HTTP MCP client: POST JSON-RPC, parse the SSE answer.
async function mcpRequest(body: unknown, token: string, sessionId?: string) {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    'Accept': 'application/json, text/event-stream',
    'Authorization': `Bearer ${token}`,
  }
  if (sessionId)
    headers['Mcp-Session-Id'] = sessionId
  const response = await fetch(ENDPOINT, { method: 'POST', headers, body: JSON.stringify(body) })
  const text = await response.text()
  // Priming events carry empty data lines: the message is the last non-empty one.
  const data = text
    .split('\n')
    .filter(line => line.startsWith('data:'))
    .map(line => line.slice(5).trim())
    .filter(line => line !== '')
    .at(-1)
  return {
    status: response.status,
    sessionId: response.headers.get('mcp-session-id') ?? sessionId,
    json: data ? JSON.parse(data) : null,
  }
}

describe('mcp server', () => {
  it('shows the panel, off by default', async () => {
    await $('[data-testid="mcp-stopped"]').waitForExist()
    await $('[data-testid="mcp-regenerate"]').waitForExist()
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

  it('talks MCP end to end with the token', async () => {
    const token = await $('[data-testid="mcp-details"]').getAttribute('data-token')
    if (token === null)
      throw new Error('panel exposes no token')

    const init = await mcpRequest({
      jsonrpc: '2.0',
      id: 1,
      method: 'initialize',
      params: {
        protocolVersion: '2025-06-18',
        capabilities: {},
        clientInfo: { name: 'soquel-e2e', version: '0.0.0' },
      },
    }, token)
    expect(init.status).toBe(200)
    expect(init.json.result.capabilities.tools).toBeDefined()

    await mcpRequest({ jsonrpc: '2.0', method: 'notifications/initialized' }, token, init.sessionId)

    const tools = await mcpRequest({ jsonrpc: '2.0', id: 2, method: 'tools/list' }, token, init.sessionId)
    const names = tools.json.result.tools.map((tool: { name: string }) => tool.name)
    expect(names).toContain('list_connections')
    expect(names).toContain('run_query')
    expect(names).toContain('get_schema')

    const call = await mcpRequest({
      jsonrpc: '2.0',
      id: 3,
      method: 'tools/call',
      params: { name: 'list_connections', arguments: {} },
    }, token, init.sessionId)
    const listed = JSON.parse(call.json.result.content[0].text)
    expect(listed.map((c: { name: string }) => c.name)).toContain('agent sqlite')
    expect(listed[0].access).toBe('read-only')
  })

  it('comes back on its own after an app restart', async () => {
    await browser.reloadSession()
    await $('[data-testid="mcp-running"]').waitForExist()
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

  it('deletes the connection', async () => {
    await deleteFirstConnection()
  })
})
