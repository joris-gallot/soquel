import type { ConnectionInput, ConnectionProfile, ConnectorKind, ConnectorParams, Env, SslMode } from '@/lib/bindings'
import { z } from 'zod'

export const ENVS = ['dev', 'staging', 'prod'] as const satisfies readonly Env[]

export const KINDS = ['postgres', 'mysql', 'sqlite', 'redis'] as const satisfies readonly ConnectorKind[]

export const KIND_META: Record<ConnectorKind, {
  label: string
  short: string
  defaultPort: number
  protocols: string[]
}> = {
  postgres: { label: 'PostgreSQL', short: 'PG', defaultPort: 5432, protocols: ['postgres:', 'postgresql:'] },
  mysql: { label: 'MySQL', short: 'MySQL', defaultPort: 3306, protocols: ['mysql:'] },
  sqlite: { label: 'SQLite', short: 'SQLite', defaultPort: 0, protocols: [] },
  redis: { label: 'Redis', short: 'Redis', defaultPort: 6379, protocols: ['redis:', 'rediss:'] },
  // Rust surface only for now: the doc browser UI round adds the form entry + mongodb: prefill.
  mongo: { label: 'MongoDB', short: 'Mongo', defaultPort: 27017, protocols: [] },
}

/// What the form's engine select shows: MariaDB is a display entry riding the
/// mysql kind (wire-compatible, quirks handled at runtime via the version).
export const ENGINE_CHOICES = [
  { id: 'postgres', label: 'PostgreSQL', kind: 'postgres' },
  { id: 'mysql', label: 'MySQL', kind: 'mysql' },
  { id: 'mariadb', label: 'MariaDB', kind: 'mysql' },
  { id: 'sqlite', label: 'SQLite', kind: 'sqlite' },
  { id: 'redis', label: 'Redis', kind: 'redis' },
] as const satisfies readonly { id: string, label: string, kind: ConnectorKind }[]

export type EngineChoice = (typeof ENGINE_CHOICES)[number]['id']

/// The form only edits kinds ENGINE_CHOICES offers; mongo arrives with its UI round.
export function engineChoiceForKind(kind: ConnectorKind): EngineChoice {
  return ENGINE_CHOICES.some(choice => choice.id === kind) ? kind as EngineChoice : 'postgres'
}

export const SSL_MODES = ['disable', 'prefer', 'require', 'verify-full'] as const satisfies readonly SslMode[]

export const NO_TUNNEL = 'none'

export const ENV_BADGE_CLASSES: Record<Env, string> = {
  dev: 'border-transparent bg-muted text-muted-foreground',
  staging: 'border-amber-500/30 bg-amber-500/10 text-amber-500',
  prod: 'border-destructive/30 bg-destructive/10 text-destructive',
}

/// Workspace badge for a live server: MariaDB announces itself through the
/// mysql kind's version string ("11.4.7-MariaDB-log").
export function serverBadge(kind: ConnectorKind, version: string): { engine: string, version: string } {
  if (kind === 'mysql' && version.includes('MariaDB'))
    return { engine: 'MariaDB', version: version.split('-')[0] }
  // Valkey announces itself through the redis kind ("8.0.1-valkey").
  if (kind === 'redis' && version.includes('valkey'))
    return { engine: 'Valkey', version: version.split('-')[0] }
  return { engine: KIND_META[kind].short, version: version.split(' ')[0] }
}

/// Follow the kind only when the port still sits on the previous kind's
/// default; a hand-set port survives engine switches. SQLite has no port:
/// switching away always lands on the next kind's default.
export function portForKindChange(
  port: number | string,
  previousKind: ConnectorKind,
  nextKind: ConnectorKind,
): number | string {
  if (nextKind === 'sqlite')
    return port
  return previousKind === 'sqlite' || Number(port) === KIND_META[previousKind].defaultPort
    ? KIND_META[nextKind].defaultPort
    : port
}

/// One-line identity for lists and the palette: DSN-ish for servers, the file
/// path for sqlite.
export function connectionTarget(params: ConnectorParams): string {
  if (params.kind === 'sqlite')
    return params.path
  if (params.kind === 'redis')
    return `${params.host}:${params.port}/${params.db}`
  if (params.kind === 'mongo')
    return `${params.host}:${params.port}${params.database ? `/${params.database}` : ''}`
  return `${params.host}:${params.port}/${params.database}`
}

export function connectionDsn(params: ConnectorParams): string {
  if (params.kind === 'sqlite')
    return `sqlite://${params.path}`
  if (params.kind === 'redis')
    return `${params.tls ? 'rediss' : 'redis'}://${params.host}:${params.port}/${params.db}`
  if (params.kind === 'mongo')
    return `mongodb://${params.username ? `${params.username}@` : ''}${params.host}:${params.port}${params.database ? `/${params.database}` : ''}`
  return `${params.kind}://${params.user}@${params.host}:${params.port}/${params.database}`
}

export const connectionSchema = z.object({
  name: z.string().min(1, 'Name is required'),
  env: z.enum(ENVS),
  kind: z.enum(KINDS),
  host: z.string(),
  port: z.coerce.number().int('Port must be a whole number').min(0).max(65535, 'Port must be below 65536'),
  database: z.string(),
  user: z.string(),
  sslMode: z.enum(SSL_MODES),
  sslRootCert: z.string(),
  // Reka Select reserves '' for clearing, so "no tunnel" is a sentinel value.
  tunnelId: z.string().catch(NO_TUNNEL),
  group: z.string(),
  password: z.string(),
  path: z.string(),
  dbIndex: z.coerce.number().int('DB index must be a whole number').min(0, 'DB index must be positive').catch(0),
  tls: z.boolean(),
}).superRefine((values, ctx) => {
  // Each kind validates its own shape; sqlite is a file, redis has no database/user.
  if (values.kind === 'sqlite') {
    if (values.path.trim() === '')
      ctx.addIssue({ code: 'custom', path: ['path'], message: 'Database file is required' })
    return
  }
  if (values.host === '')
    ctx.addIssue({ code: 'custom', path: ['host'], message: 'Host is required' })
  if (values.port < 1)
    ctx.addIssue({ code: 'custom', path: ['port'], message: 'Port is required' })
  if (values.kind === 'redis')
    return
  if (values.database === '')
    ctx.addIssue({ code: 'custom', path: ['database'], message: 'Database is required' })
  if (values.user === '')
    ctx.addIssue({ code: 'custom', path: ['user'], message: 'User is required' })
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
  path: string
  // Redis only: numeric database index and plain TLS toggle.
  dbIndex: number | string
  tls: boolean
}

export function toConnectionInput(values: z.output<typeof connectionSchema>): ConnectionInput {
  const base = {
    name: values.name,
    env: values.env,
    group: values.group.trim() === '' ? null : values.group.trim(),
    password: values.password === '' ? null : values.password,
  }
  if (values.kind === 'sqlite')
    return { ...base, password: null, params: { kind: 'sqlite', path: values.path.trim() } }
  if (values.kind === 'redis') {
    return {
      ...base,
      params: {
        kind: 'redis',
        host: values.host,
        port: values.port,
        db: values.dbIndex,
        username: values.user.trim() === '' ? null : values.user.trim(),
        tls: values.tls,
        tunnelId: values.tunnelId === NO_TUNNEL || values.tunnelId === '' ? null : values.tunnelId,
      },
    }
  }
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
    ...base,
    params: values.kind === 'postgres'
      ? { kind: 'postgres', ...shared }
      : { kind: 'mysql', ...shared },
  }
}

/// Flatten a stored profile back into the form's editable shape.
export function formValuesFromProfile(profile: ConnectionProfile): ConnectionFormValues {
  const params = profile.params
  const base = {
    name: profile.name,
    env: profile.env,
    kind: params.kind,
    group: profile.group ?? '',
    password: '',
  }
  const defaults = {
    host: 'localhost',
    port: 0,
    database: '',
    user: '',
    sslMode: 'prefer' as SslMode,
    sslRootCert: '',
    tunnelId: NO_TUNNEL,
    path: '',
    dbIndex: 0,
    tls: false,
  }
  if (params.kind === 'sqlite')
    return { ...base, ...defaults, path: params.path }
  if (params.kind === 'redis') {
    return {
      ...base,
      ...defaults,
      host: params.host,
      port: params.port,
      user: params.username ?? '',
      dbIndex: params.db ?? 0,
      tls: params.tls ?? false,
      tunnelId: params.tunnelId ?? NO_TUNNEL,
    }
  }
  if (params.kind === 'mongo') {
    return {
      ...base,
      ...defaults,
      host: params.host,
      port: params.port,
      database: params.database ?? '',
      user: params.username ?? '',
      tls: params.tls ?? false,
      tunnelId: params.tunnelId ?? NO_TUNNEL,
    }
  }
  return {
    ...base,
    ...defaults,
    host: params.host,
    port: params.port,
    database: params.database,
    user: params.user,
    sslMode: params.sslMode ?? 'prefer',
    sslRootCert: params.sslRootCert ?? '',
    tunnelId: params.tunnelId ?? NO_TUNNEL,
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
  if (kind === 'redis') {
    // The path is a numeric db index, not a database name.
    parsed.database = ''
    parsed.dbIndex = Number(url.pathname.replace(/^\//, '')) || 0
    parsed.tls = url.protocol === 'rediss:'
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
