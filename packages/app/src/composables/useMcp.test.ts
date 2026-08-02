import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useMcp } from './useMcp'

const calls: string[] = []
const server = { running: false, port: 52700 }

function snapshot() {
  return {
    running: server.running,
    port: server.port,
    endpoint: `http://127.0.0.1:${server.port}/mcp`,
    token: 'token',
    serverName: 'soquel',
  }
}

vi.mock('@/lib/bindings', () => ({
  commands: {
    mcpStatus: vi.fn(async () => ({ status: 'ok', data: snapshot() })),
    mcpStart: vi.fn(async () => {
      calls.push('start')
      server.running = true
      return { status: 'ok', data: snapshot() }
    }),
    mcpStop: vi.fn(async () => {
      calls.push('stop')
      server.running = false
      return { status: 'ok', data: null }
    }),
    mcpSetPort: vi.fn(async (port: number) => {
      calls.push(`setPort:${port}`)
      server.port = port
      return { status: 'ok', data: snapshot() }
    }),
  },
}))

describe('useMcp', () => {
  beforeEach(async () => {
    server.running = false
    server.port = 52700
    await useMcp().refresh()
    calls.length = 0
  })

  it('only persists the port while the server is stopped', async () => {
    const { setPort, status } = useMcp()
    await setPort(52799)

    expect(calls).toEqual(['setPort:52799'])
    expect(status.value?.running).toBe(false)
    expect(status.value?.port).toBe(52799)
  })

  it('persists before restarting, so a port that fails to bind is still the one on disk', async () => {
    const { start, setPort, status } = useMcp()
    await start()
    calls.length = 0

    await setPort(52799)

    expect(calls).toEqual(['stop', 'setPort:52799', 'start'])
    expect(status.value?.running).toBe(true)
    expect(status.value?.endpoint).toBe('http://127.0.0.1:52799/mcp')
  })
})
