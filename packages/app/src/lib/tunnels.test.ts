import { describe, expect, it } from 'vitest'
import { toTunnelInput, tunnelSchema } from './tunnels'
import { zodFieldErrors } from './validation'

describe('tunnelSchema', () => {
  const valid = {
    name: 'bastion',
    host: 'bastion.internal',
    port: '22',
    user: 'deploy',
    method: 'agent',
    keyPath: '',
    secret: '',
  }

  it('coerces the port and accepts agent auth without a key path', () => {
    const parsed = tunnelSchema.parse(valid)
    expect(parsed.port).toBe(22)
  })

  it('requires a key path for key-file auth', () => {
    const result = tunnelSchema.safeParse({ ...valid, method: 'key-file' })
    expect(result.success).toBe(false)
    if (!result.success)
      expect(zodFieldErrors(result.error).keyPath).toBe('Key path is required')
  })

  it('builds the tagged auth for the command input', () => {
    const agent = toTunnelInput(tunnelSchema.parse(valid))
    expect(agent.auth).toEqual({ method: 'agent' })
    expect(agent.secret).toBeNull()

    const keyFile = toTunnelInput(tunnelSchema.parse({
      ...valid,
      method: 'key-file',
      keyPath: '~/.ssh/id_ed25519',
      secret: 'passphrase',
    }))
    expect(keyFile.auth).toEqual({ method: 'key-file', path: '~/.ssh/id_ed25519' })
    expect(keyFile.secret).toBe('passphrase')

    const password = toTunnelInput(tunnelSchema.parse({ ...valid, method: 'password', secret: 'pw' }))
    expect(password.auth).toEqual({ method: 'password' })
    expect(password.secret).toBe('pw')
  })

  it('drops a stale secret when the method carries no credential', () => {
    const none = toTunnelInput(tunnelSchema.parse({ ...valid, method: 'none', secret: 'leftover' }))
    expect(none.auth).toEqual({ method: 'none' })
    expect(none.secret).toBeNull()
  })
})
