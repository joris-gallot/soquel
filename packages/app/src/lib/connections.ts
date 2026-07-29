import type { ConnectionInput, Env, SslMode } from '@/lib/bindings'
import { z } from 'zod'

export const ENVS = ['dev', 'staging', 'prod'] as const satisfies readonly Env[]

export const SSL_MODES = ['disable', 'prefer', 'require', 'verify-full'] as const satisfies readonly SslMode[]

export const ENV_BADGE_CLASSES: Record<Env, string> = {
  dev: 'border-transparent bg-muted text-muted-foreground',
  staging: 'border-amber-500/30 bg-amber-500/10 text-amber-500',
  prod: 'border-destructive/30 bg-destructive/10 text-destructive',
}

export const connectionSchema = z.object({
  name: z.string().min(1, 'Name is required'),
  env: z.enum(ENVS),
  kind: z.literal('postgres'),
  host: z.string().min(1, 'Host is required'),
  port: z.coerce.number().int('Port must be a whole number').min(1, 'Port is required').max(65535, 'Port must be below 65536'),
  database: z.string().min(1, 'Database is required'),
  user: z.string().min(1, 'User is required'),
  sslMode: z.enum(SSL_MODES),
  password: z.string(),
})

export interface ConnectionFormValues {
  name: string
  env: Env
  kind: 'postgres'
  host: string
  // Bound to a text input: coerced by the schema on submit.
  port: number | string
  database: string
  user: string
  sslMode: SslMode
  password: string
}

export function toConnectionInput(values: z.output<typeof connectionSchema>): ConnectionInput {
  return { ...values, password: values.password === '' ? null : values.password }
}

// libpq's sslmode values mapped onto the app's coarser set.
type LibpqSslMode = 'disable' | 'allow' | 'prefer' | 'require' | 'verify-ca' | 'verify-full'

const URL_SSL_MODES: Record<LibpqSslMode, SslMode> = {
  'disable': 'disable',
  'allow': 'prefer',
  'prefer': 'prefer',
  'require': 'require',
  'verify-ca': 'verify-full',
  'verify-full': 'verify-full',
}

/// Prefill form values from a postgres:// or postgresql:// URL.
export function parsePostgresUrl(raw: string): Partial<ConnectionFormValues> | null {
  let url: URL
  try {
    url = new URL(raw.trim())
  }
  catch {
    return null
  }
  if (url.protocol !== 'postgres:' && url.protocol !== 'postgresql:')
    return null
  const parsed: Partial<ConnectionFormValues> = {
    host: url.hostname || 'localhost',
    port: url.port === '' ? 5432 : Number(url.port),
    database: decodeURIComponent(url.pathname.replace(/^\//, '')),
    user: decodeURIComponent(url.username),
    password: decodeURIComponent(url.password),
  }
  const sslmode = url.searchParams.get('sslmode')
  if (sslmode && sslmode in URL_SSL_MODES)
    parsed.sslMode = URL_SSL_MODES[sslmode as LibpqSslMode]
  return parsed
}
