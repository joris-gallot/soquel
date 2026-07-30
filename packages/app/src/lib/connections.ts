import type { ConnectionInput, ConnectionProfile, ConnectorKind, Env, SslMode } from '@/lib/bindings'
import { z } from 'zod'

export const ENVS = ['dev', 'staging', 'prod'] as const satisfies readonly Env[]

export const KINDS = ['postgres', 'mysql'] as const satisfies readonly ConnectorKind[]

export const KIND_META: Record<ConnectorKind, {
  label: string
  short: string
  defaultPort: number
  protocols: string[]
}> = {
  postgres: { label: 'PostgreSQL', short: 'PG', defaultPort: 5432, protocols: ['postgres:', 'postgresql:'] },
  mysql: { label: 'MySQL', short: 'MySQL', defaultPort: 3306, protocols: ['mysql:'] },
}

export const SSL_MODES = ['disable', 'prefer', 'require', 'verify-full'] as const satisfies readonly SslMode[]

export const NO_TUNNEL = 'none'

export const ENV_BADGE_CLASSES: Record<Env, string> = {
  dev: 'border-transparent bg-muted text-muted-foreground',
  staging: 'border-amber-500/30 bg-amber-500/10 text-amber-500',
  prod: 'border-destructive/30 bg-destructive/10 text-destructive',
}

/// Follow the kind only when the port still sits on the previous kind's
/// default; a hand-set port survives engine switches.
export function portForKindChange(
  port: number | string,
  previousKind: ConnectorKind,
  nextKind: ConnectorKind,
): number | string {
  return Number(port) === KIND_META[previousKind].defaultPort
    ? KIND_META[nextKind].defaultPort
    : port
}

export const connectionSchema = z.object({
  name: z.string().min(1, 'Name is required'),
  env: z.enum(ENVS),
  kind: z.enum(KINDS),
  host: z.string().min(1, 'Host is required'),
  port: z.coerce.number().int('Port must be a whole number').min(1, 'Port is required').max(65535, 'Port must be below 65536'),
  database: z.string().min(1, 'Database is required'),
  user: z.string().min(1, 'User is required'),
  sslMode: z.enum(SSL_MODES),
  sslRootCert: z.string(),
  // Reka Select reserves '' for clearing, so "no tunnel" is a sentinel value.
  tunnelId: z.string().catch(NO_TUNNEL),
  group: z.string(),
  password: z.string(),
})

export interface ConnectionFormValues {
  name: string
  env: Env
  kind: ConnectorKind
  host: string
  // Bound to a text input: coerced by the schema on submit.
  port: number | string
  database: string
  user: string
  sslMode: SslMode
  sslRootCert: string
  tunnelId: string
  group: string
  password: string
}

export function toConnectionInput(values: z.output<typeof connectionSchema>): ConnectionInput {
  const shared = {
    host: values.host,
    port: values.port,
    database: values.database,
    user: values.user,
    sslMode: values.sslMode,
    // The CA only applies to verify-full: don't persist a stale path for other modes.
    sslRootCert: values.sslMode === 'verify-full' && values.sslRootCert.trim() !== ''
      ? values.sslRootCert.trim()
      : null,
    tunnelId: values.tunnelId === NO_TUNNEL || values.tunnelId === '' ? null : values.tunnelId,
  }
  return {
    name: values.name,
    env: values.env,
    group: values.group.trim() === '' ? null : values.group.trim(),
    password: values.password === '' ? null : values.password,
    params: values.kind === 'postgres'
      ? { kind: 'postgres', ...shared }
      : { kind: 'mysql', ...shared },
  }
}

/// Flatten a stored profile back into the form's editable shape.
export function formValuesFromProfile(profile: ConnectionProfile): ConnectionFormValues {
  const params = profile.params
  return {
    name: profile.name,
    env: profile.env,
    kind: params.kind,
    host: params.host,
    port: params.port,
    database: params.database,
    user: params.user,
    sslMode: params.sslMode ?? 'prefer',
    sslRootCert: params.sslRootCert ?? '',
    tunnelId: params.tunnelId ?? NO_TUNNEL,
    group: profile.group ?? '',
    password: '',
  }
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

// mysql's ssl-mode vocabulary mapped onto the app's set.
const MYSQL_URL_SSL_MODES: Record<string, SslMode> = {
  DISABLED: 'disable',
  PREFERRED: 'prefer',
  REQUIRED: 'require',
  VERIFY_CA: 'verify-full',
  VERIFY_IDENTITY: 'verify-full',
}

/// Prefill form values from a postgres:// / postgresql:// / mysql:// URL.
export function parseConnectionUrl(raw: string): Partial<ConnectionFormValues> | null {
  let url: URL
  try {
    url = new URL(raw.trim())
  }
  catch {
    return null
  }
  const kind = KINDS.find(candidate => KIND_META[candidate].protocols.includes(url.protocol))
  if (!kind)
    return null
  const parsed: Partial<ConnectionFormValues> = {
    kind,
    host: url.hostname || 'localhost',
    port: url.port === '' ? KIND_META[kind].defaultPort : Number(url.port),
    database: decodeURIComponent(url.pathname.replace(/^\//, '')),
    user: decodeURIComponent(url.username),
    password: decodeURIComponent(url.password),
  }
  const sslmode = url.searchParams.get('sslmode')
  if (sslmode && sslmode in URL_SSL_MODES)
    parsed.sslMode = URL_SSL_MODES[sslmode as LibpqSslMode]
  const mysqlSslMode = url.searchParams.get('ssl-mode')?.toUpperCase()
  if (mysqlSslMode && mysqlSslMode in MYSQL_URL_SSL_MODES)
    parsed.sslMode = MYSQL_URL_SSL_MODES[mysqlSslMode]
  const sslrootcert = url.searchParams.get('sslrootcert')
  if (sslrootcert)
    parsed.sslRootCert = sslrootcert
  return parsed
}

export interface ConnectionSection {
  group: string | null
  profiles: ConnectionProfile[]
}

/// Ungrouped first, then groups alphabetically; stored order within a section.
export function groupConnections(profiles: ConnectionProfile[]): ConnectionSection[] {
  const sections = new Map<string | null, ConnectionProfile[]>()
  for (const profile of profiles) {
    const key = profile.group ?? null
    const bucket = sections.get(key) ?? []
    bucket.push(profile)
    sections.set(key, bucket)
  }
  const named = [...sections.keys()]
    .filter((group): group is string => group !== null)
    .sort((a, b) => a.localeCompare(b))
  const order: (string | null)[] = sections.has(null) ? [null, ...named] : named
  return order.map(group => ({ group, profiles: sections.get(group)! }))
}
