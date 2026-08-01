import type { SshAuth, TunnelInput, TunnelProfile } from '@/lib/bindings'
import type { CredentialMode } from '@/lib/connections'
import { z } from 'zod'
import { CREDENTIAL_MODES, credentialSourceFromValues } from '@/lib/connections'

export type SshAuthMethod = SshAuth['method']

export const SSH_AUTH_METHODS = ['agent', 'key-file', 'password', 'none'] as const satisfies readonly SshAuthMethod[]

export const SSH_AUTH_LABELS: Record<SshAuthMethod, string> = {
  'agent': 'SSH agent',
  'key-file': 'Key file',
  'password': 'Password',
  'none': 'None',
}

export const SSH_AUTH_HINTS: Record<SshAuthMethod, string | null> = {
  'agent': null,
  'key-file': null,
  'password': null,
  'none': 'No credential is sent: the server authorizes the connection on its own.',
}

/// Methods whose credential lives in the SecretStore.
export const SSH_AUTH_NEEDS_SECRET: Record<SshAuthMethod, boolean> = {
  'agent': false,
  'key-file': true,
  'password': true,
  'none': false,
}

export const tunnelSchema = z.object({
  name: z.string().min(1, 'Name is required'),
  host: z.string().min(1, 'Host is required'),
  port: z.coerce.number().int('Port must be a whole number').min(1, 'Port is required').max(65535, 'Port must be below 65536'),
  user: z.string().min(1, 'User is required'),
  method: z.enum(SSH_AUTH_METHODS),
  keyPath: z.string(),
  secret: z.string(),
  credentialMode: z.enum(CREDENTIAL_MODES),
  credentialCommand: z.string(),
}).superRefine((values, ctx) => {
  if (values.method === 'key-file' && values.keyPath.length === 0)
    ctx.addIssue({ code: 'custom', path: ['keyPath'], message: 'Key path is required' })
  if (SSH_AUTH_NEEDS_SECRET[values.method] && values.credentialMode === 'command' && values.credentialCommand.trim() === '')
    ctx.addIssue({ code: 'custom', path: ['credentialCommand'], message: 'Command is required' })
})

export interface TunnelFormValues {
  name: string
  host: string
  // Bound to a text input: coerced by the schema on submit.
  port: number | string
  user: string
  method: SshAuthMethod
  keyPath: string
  secret: string
  credentialMode: CredentialMode
  credentialCommand: string
}

export function toTunnelInput(values: z.output<typeof tunnelSchema>): TunnelInput {
  const auth: SshAuth = values.method === 'key-file'
    ? { method: 'key-file', path: values.keyPath }
    : { method: values.method }
  const needsSecret = SSH_AUTH_NEEDS_SECRET[values.method]
  const secret = needsSecret ? values.secret : ''
  return {
    name: values.name,
    host: values.host,
    port: values.port,
    user: values.user,
    auth,
    // An agent or a credential-less server has no secret of ours to source.
    credential: needsSecret ? credentialSourceFromValues(values) : { mode: 'keychain' },
    secret: secret === '' ? null : secret,
  }
}

/// Flatten a stored tunnel back into the form's editable shape.
export function tunnelFormValues(tunnel: TunnelProfile): TunnelFormValues {
  return {
    name: tunnel.name,
    host: tunnel.host,
    port: tunnel.port,
    user: tunnel.user,
    method: tunnel.auth.method,
    keyPath: tunnel.auth.method === 'key-file' ? tunnel.auth.path : '',
    secret: '',
    credentialMode: tunnel.credential?.mode ?? 'keychain',
    credentialCommand: tunnel.credential?.mode === 'command' ? tunnel.credential.command : '',
  }
}
