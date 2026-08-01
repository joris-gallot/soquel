import type { TunnelProfile } from '@/lib/bindings'
import { describe, expect, it } from 'vitest'
import { toTunnelInput, tunnelFormValues, tunnelSchema } from './tunnels'
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
    credentialMode: 'keychain',
    credentialCommand: '',
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

  it('maps the credential mode, and pins keychain when no secret is ours to source', () => {
    const password = { ...valid, method: 'password' }
    expect(toTunnelInput(tunnelSchema.parse(password)).credential).toEqual({ mode: 'keychain' })
    expect(toTunnelInput(tunnelSchema.parse({ ...password, credentialMode: 'prompt' })).credential)
      .toEqual({ mode: 'prompt' })

    const fromCommand = { ...password, credentialMode: 'command', credentialCommand: ' vault-ssh {host} ' }
    expect(toTunnelInput(tunnelSchema.parse(fromCommand)).credential)
      .toEqual({ mode: 'command', command: 'vault-ssh {host}', refreshAfterSecs: null })

    // An agent holds the key: a mode left over from another method is dropped.
    expect(toTunnelInput(tunnelSchema.parse({ ...fromCommand, method: 'agent' })).credential)
      .toEqual({ mode: 'keychain' })
  })

  it('requires a command only when the method has a secret to source', () => {
    const missing = tunnelSchema.safeParse({ ...valid, method: 'password', credentialMode: 'command' })
    expect(missing.success).toBe(false)
    if (!missing.success)
      expect(zodFieldErrors(missing.error).credentialCommand).toBe('Command is required')

    expect(tunnelSchema.safeParse({ ...valid, method: 'agent', credentialMode: 'command' }).success).toBe(true)
  })
})

describe('tunnelFormValues', () => {
  const stored: TunnelProfile = {
    id: 't-1',
    name: 'bastion',
    host: 'bastion.internal',
    port: 2222,
    user: 'deploy',
    auth: { method: 'password' },
  }

  it('reads the mode back; a tunnel saved before the modes reads as keychain', () => {
    expect(tunnelFormValues(stored)).toMatchObject({ credentialMode: 'keychain', credentialCommand: '' })

    const fromCommand = {
      ...stored,
      credential: { mode: 'command' as const, command: 'vault-ssh {host}', refreshAfterSecs: null },
    }
    expect(tunnelFormValues(fromCommand)).toMatchObject({
      credentialMode: 'command',
      credentialCommand: 'vault-ssh {host}',
    })
  })

  it('never carries the stored secret back into the form', () => {
    expect(tunnelFormValues(stored).secret).toBe('')
  })
})
