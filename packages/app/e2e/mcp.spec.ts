import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { $, $$, browser, expect } from '@wdio/globals'
import { createSqliteConnection, deleteFirstConnection } from './helpers'

// The e2e binary is a debug build: dev port, isolated from an installed app.
const ENDPOINT = 'http://127.0.0.1:52701/mcp'
// A zero-byte file is a valid empty sqlite database.
const DB_PATH = path.join(os.tmpdir(), 'soquel-e2e-agent.sqlite')

/// Set by the handshake test; the write tests address the same connection.
let connectionId = ''

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

/// initialize + initialized: every later call rides the returned session.
async function handshake(token: string) {
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
  await mcpRequest({ jsonrpc: '2.0', method: 'notifications/initialized' }, token, init.sessionId)
  return init.sessionId
}

describe('mcp server', () => {
  before(() => {
    for (const suffix of ['', '-wal', '-shm'])
      fs.rmSync(`${DB_PATH}${suffix}`, { force: true })
    fs.writeFileSync(DB_PATH, '')
  })

  after(() => {
    for (const suffix of ['', '-wal', '-shm'])
      fs.rmSync(`${DB_PATH}${suffix}`, { force: true })
  })

  it('shows the panel, off by default', async () => {
    await $('[data-testid="mcp-stopped"]').waitForExist()
    await $('[data-testid="mcp-regenerate"]').waitForExist()
  })

  it('badges connections opted in for agents', async () => {
    await createSqliteConnection('agent sqlite', DB_PATH)
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

    const session = await handshake(token)
    const tools = await mcpRequest({ jsonrpc: '2.0', id: 2, method: 'tools/list' }, token, session)
    const names = tools.json.result.tools.map((tool: { name: string }) => tool.name)
    expect(names).toContain('list_connections')
    expect(names).toContain('run_query')
    expect(names).toContain('get_schema')

    const call = await mcpRequest({
      jsonrpc: '2.0',
      id: 3,
      method: 'tools/call',
      params: { name: 'list_connections', arguments: {} },
    }, token, session)
    const listed = JSON.parse(call.json.result.content[0].text)
    expect(listed.map((c: { name: string }) => c.name)).toContain('agent sqlite')
    expect(listed[0].access).toBe('read-only')
    connectionId = listed[0].id
  })

  it('refuses writes on a read-only connection', async () => {
    const token = await $('[data-testid="mcp-details"]').getAttribute('data-token')
    const session = await handshake(token!)
    const call = await mcpRequest({
      jsonrpc: '2.0',
      id: 10,
      method: 'tools/call',
      params: {
        name: 'run_query',
        arguments: { connection_id: connectionId, sql: 'CREATE TABLE leaked (id integer)' },
      },
    }, token!, session)
    expect(call.json.error.message).toContain('read-only for agents')
  })

  it('asks before a write, and runs it once allowed', async () => {
    const token = await $('[data-testid="mcp-details"]').getAttribute('data-token')

    // Upgrade the connection to write-with-approval.
    await $('[data-testid="row-menu"]').click()
    await $('[data-testid="row-edit"]').click()
    await $('[data-testid="field-agent-access"]').click()
    await $('[data-testid="agent-access-write-with-approval"]').click()
    await $('[data-testid="save-connection"]').click()
    await $('[data-testid="field-agent-access"]').waitForExist({ reverse: true })

    const session = await handshake(token!)
    const write = (id: number, sql: string) => mcpRequest({
      jsonrpc: '2.0',
      id,
      method: 'tools/call',
      params: { name: 'run_query', arguments: { connection_id: connectionId, sql } },
    }, token!, session)

    // Denied: the dialog answers no, the agent gets refused.
    const denied = write(11, 'CREATE TABLE denied_probe (id integer)')
    await $('[data-testid="approval-dialog"]').waitForExist()
    // Past the open animation, so the shot shows the settled dialog.
    await $('[data-testid="approval-deny"]').waitForClickable()
    await browser.saveScreenshot('./e2e/screenshots/mcp-approval.png')
    await $('[data-testid="approval-deny"]').click()
    expect((await denied).json.error.message).toContain('not approved')

    // Allowed: the same statement lands.
    const allowed = write(12, 'CREATE TABLE allowed_probe (id integer)')
    await $('[data-testid="approval-dialog"]').waitForExist()
    await expect($('[data-testid="approval-sql"]')).toHaveText('CREATE TABLE allowed_probe (id integer)')
    await $('[data-testid="approval-allow"]').click()
    expect((await allowed).json.result).toBeDefined()

    const check = await write(13, 'SELECT name FROM sqlite_master WHERE type = \'table\'')
    const tables = JSON.parse(check.json.result.content[0].text)
    const names = tables.result.statements[0].rows.flat()
    expect(names).toContain('allowed_probe')
    expect(names).not.toContain('denied_probe')
  })

  it('lists agent activity in the audit log', async () => {
    await $('[data-testid="open-audit"]').click()
    await $('[data-testid="audit-dialog"]').waitForExist()
    const rows = await $$('[data-testid="audit-row"]')
    expect(rows.length).toBeGreaterThan(0)
    await browser.saveScreenshot('./e2e/screenshots/mcp-audit.png')
    await browser.keys(['Escape'])
    await $('[data-testid="audit-dialog"]').waitForExist({ reverse: true })
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
