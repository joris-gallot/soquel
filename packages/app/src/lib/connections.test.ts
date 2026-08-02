import type { ConnectionProfile } from '@/lib/bindings'
import { describe, expect, it } from 'vitest'
import { connectionDsn, connectionSchema, connectionTarget, CREDENTIAL_COMMAND_CAVEATS, formValuesFromProfile, groupConnections, parseConnectionUrl, portForKindChange, serverBadge, toConnectionInput } from './connections'
import { zodFieldErrors } from './validation'

function profile(name: string, group: string | null): ConnectionProfile {
  return {
    id: name,
    name,
    env: 'dev',
    group,
    params: {
      kind: 'postgres',
      host: 'h',
      port: 5432,
      database: 'db',
      user: 'u',
      sslMode: 'prefer',
      sslRootCert: null,
      tunnelId: null,
    },
  }
}

describe('groupConnections', () => {
  it('puts ungrouped first, then groups alphabetically', () => {
    const sections = groupConnections([
      profile('c1', 'zeta'),
      profile('c2', null),
      profile('c3', 'alpha'),
      profile('c4', 'zeta'),
    ])
    expect(sections.map(s => s.group)).toEqual([null, 'alpha', 'zeta'])
    expect(sections[2].profiles.map(p => p.name)).toEqual(['c1', 'c4'])
  })

  it('omits the ungrouped section when everything is grouped', () => {
    expect(groupConnections([profile('c1', 'a')]).map(s => s.group)).toEqual(['a'])
  })
})

describe('parseConnectionUrl', () => {
  it('parses a full postgres:// url', () => {
    expect(parseConnectionUrl('postgres://joris:s3cret@db.internal:5433/analytics')).toEqual({
      kind: 'postgres',
      host: 'db.internal',
      port: 5433,
      database: 'analytics',
      user: 'joris',
      password: 's3cret',
    })
  })

  it('parses a mysql:// url with its own defaults and ssl vocabulary', () => {
    expect(parseConnectionUrl('mysql://u:p@db.internal/app')).toMatchObject({
      kind: 'mysql',
      port: 3306,
    })
    expect(parseConnectionUrl('mysql://u:p@h/app?ssl-mode=VERIFY_IDENTITY'))
      .toMatchObject({ sslMode: 'verify-full' })
    expect(parseConnectionUrl('mysql://u:p@h/app?ssl-mode=required'))
      .toMatchObject({ sslMode: 'require' })
  })

  it('accepts the postgresql:// protocol', () => {
    expect(parseConnectionUrl('postgresql://u@h/db')).toMatchObject({ host: 'h', user: 'u' })
  })

  it('defaults the port to 5432 when absent', () => {
    expect(parseConnectionUrl('postgres://u:p@host/db')).toMatchObject({ port: 5432 })
  })

  it('decodes percent-encoded credentials and database', () => {
    expect(parseConnectionUrl('postgres://user%40corp:p%40ss@h:5432/my%20db')).toMatchObject({
      user: 'user@corp',
      password: 'p@ss',
      database: 'my db',
    })
  })

  it('rejects other protocols', () => {
    // srv needs DNS discovery the single-node connector doesn't do.
    expect(parseConnectionUrl('mongodb+srv://u:p@cluster0.example.net/app')).toBeNull()
    expect(parseConnectionUrl('ftp://h/x')).toBeNull()
  })

  it('rejects garbage', () => {
    expect(parseConnectionUrl('not a url')).toBeNull()
  })

  it('maps the sslmode query param onto the app modes', () => {
    expect(parseConnectionUrl('postgres://u:p@h/db?sslmode=require')).toMatchObject({ sslMode: 'require' })
    expect(parseConnectionUrl('postgres://u:p@h/db?sslmode=verify-ca')).toMatchObject({ sslMode: 'verify-full' })
    expect(parseConnectionUrl('postgres://u:p@h/db?sslmode=allow')).toMatchObject({ sslMode: 'prefer' })
  })

  it('leaves sslMode untouched when absent or unknown', () => {
    expect(parseConnectionUrl('postgres://u:p@h/db')).not.toHaveProperty('sslMode')
    expect(parseConnectionUrl('postgres://u:p@h/db?sslmode=nonsense')).not.toHaveProperty('sslMode')
  })

  it('carries sslrootcert through', () => {
    expect(parseConnectionUrl('postgres://u:p@h/db?sslmode=verify-full&sslrootcert=/etc/ca.pem'))
      .toMatchObject({ sslMode: 'verify-full', sslRootCert: '/etc/ca.pem' })
  })
})

describe('formValuesFromProfile', () => {
  it('roundtrips a fully loaded profile through the form without losing fields', () => {
    const stored: ConnectionProfile = {
      id: 'c-1',
      name: 'prod replica',
      env: 'prod',
      group: 'clients',
      params: {
        kind: 'mysql',
        host: 'db.internal',
        port: 3307,
        database: 'app',
        user: 'reader',
        sslMode: 'verify-full',
        sslRootCert: '/etc/ca.pem',
        tunnelId: 't-1',
      },
    }
    // Edit dialog opens (profile -> form), user saves untouched (form -> input):
    // every stored field must survive the trip.
    const input = toConnectionInput(connectionSchema.parse(formValuesFromProfile(stored)))
    expect(input.params).toEqual(stored.params)
    expect(input.name).toBe(stored.name)
    expect(input.env).toBe(stored.env)
    expect(input.group).toBe(stored.group)
    expect(input.password).toBeNull()
  })

  it('maps stored nulls onto the form sentinels', () => {
    const values = formValuesFromProfile(profile('bare', null))
    expect(values.tunnelId).toBe('none')
    expect(values.sslRootCert).toBe('')
    expect(values.group).toBe('')
  })

  it('carries agent access into the input; pre-field profiles read as off', () => {
    const values = formValuesFromProfile(profile('bare', null))
    expect(values.agentAccess).toBe('none')
    const input = toConnectionInput(connectionSchema.parse({ ...values, agentAccess: 'read-only' }))
    expect(input.agentAccess).toBe('read-only')
  })

  it('reads the credential mode back into the form; pre-field profiles read as keychain', () => {
    expect(formValuesFromProfile(profile('bare', null))).toMatchObject({
      credentialMode: 'keychain',
      credentialCommand: '',
    })

    const stored: ConnectionProfile = {
      ...profile('iam', null),
      credential: { mode: 'command', command: 'aws rds token {host}', refreshAfterSecs: 60 },
    }
    expect(formValuesFromProfile(stored)).toMatchObject({
      credentialMode: 'command',
      credentialCommand: 'aws rds token {host}',
    })
  })
})

describe('serverBadge', () => {
  it('splits engines and cleans version strings per flavor', () => {
    expect(serverBadge('postgres', '18.4 (Debian 18.4-1.pgdg120+1)'))
      .toEqual({ engine: 'PG', version: '18.4' })
    expect(serverBadge('mysql', '8.4.11')).toEqual({ engine: 'MySQL', version: '8.4.11' })
    expect(serverBadge('mysql', '11.4.7-MariaDB-log'))
      .toEqual({ engine: 'MariaDB', version: '11.4.7' })
  })
})

describe('portForKindChange', () => {
  it('follows the kind while the port sits on the previous default', () => {
    expect(portForKindChange(5432, 'postgres', 'mysql')).toBe(3306)
    expect(portForKindChange(3306, 'mysql', 'postgres')).toBe(5432)
    // Text input delivers strings: the comparison must still match.
    expect(portForKindChange('5432', 'postgres', 'mysql')).toBe(3306)
  })

  it('never touches a hand-set port', () => {
    expect(portForKindChange(5471, 'postgres', 'mysql')).toBe(5471)
    expect(portForKindChange('5471', 'mysql', 'postgres')).toBe('5471')
  })
})

describe('credential command caveats', () => {
  // Redis and Mongo hold the credential for the connection's life; the SQL
  // pools re-resolve it. Teaching one of them to refresh means dropping its line.
  it('warns for the connectors that cannot replay auth, and only those', () => {
    const warned = Object.entries(CREDENTIAL_COMMAND_CAVEATS)
      .filter(([, caveat]) => caveat !== null)
      .map(([kind]) => kind)

    expect(warned.sort()).toEqual(['mongo', 'redis'])
  })
})

describe('connectionSchema', () => {
  const valid = {
    name: 'local',
    env: 'dev',
    kind: 'postgres',
    agentAccess: 'none',
    host: 'localhost',
    port: '5432',
    database: 'app',
    user: 'postgres',
    sslMode: 'prefer',
    sslRootCert: '',
    tunnelId: '',
    group: '',
    password: '',
    credentialMode: 'keychain',
    credentialCommand: '',
    path: '',
    dbIndex: 0,
    tls: false,
    authSource: '',
  }

  it('coerces the port from a text input', () => {
    const parsed = connectionSchema.parse(valid)
    expect(parsed.port).toBe(5432)
  })

  it('maps missing required fields to per-field messages', () => {
    const result = connectionSchema.safeParse({ ...valid, name: '', host: '', database: '' })
    expect(result.success).toBe(false)
    if (!result.success) {
      const fields = zodFieldErrors(result.error)
      expect(fields.name).toBe('Name is required')
      expect(fields.host).toBe('Host is required')
      expect(fields.database).toBe('Database is required')
    }
  })

  it('rejects an out-of-range port', () => {
    const result = connectionSchema.safeParse({ ...valid, port: '70000' })
    expect(result.success).toBe(false)
  })

  it('turns an empty password into null for the command input', () => {
    const input = toConnectionInput(connectionSchema.parse(valid))
    expect(input.password).toBeNull()
    const withPassword = toConnectionInput(connectionSchema.parse({ ...valid, password: 'x' }))
    expect(withPassword.password).toBe('x')
  })

  it('trims the group and turns blank into null', () => {
    expect(toConnectionInput(connectionSchema.parse(valid)).group).toBeNull()
    expect(toConnectionInput(connectionSchema.parse({ ...valid, group: '  clients ' })).group).toBe('clients')
  })

  it('keeps the ca path only for verify-full', () => {
    const verifyFull = { ...valid, sslMode: 'verify-full', sslRootCert: ' /etc/ca.pem ' }
    expect(toConnectionInput(connectionSchema.parse(verifyFull)).params).toMatchObject({ sslRootCert: '/etc/ca.pem' })
    // Any other mode drops the stale path instead of persisting it.
    const require = { ...valid, sslMode: 'require', sslRootCert: '/etc/ca.pem' }
    expect(toConnectionInput(connectionSchema.parse(require)).params).toMatchObject({ sslRootCert: null })
    expect(toConnectionInput(connectionSchema.parse(valid)).params).toMatchObject({ sslRootCert: null })
  })

  it('turns the no-tunnel sentinel into null for the command input', () => {
    expect(toConnectionInput(connectionSchema.parse(valid)).params).toMatchObject({ tunnelId: null })
    expect(toConnectionInput(connectionSchema.parse({ ...valid, tunnelId: 'none' })).params).toMatchObject({ tunnelId: null })
    // Reka Select can clear the model to undefined; the schema falls back to no tunnel.
    expect(toConnectionInput(connectionSchema.parse({ ...valid, tunnelId: undefined })).params).toMatchObject({ tunnelId: null })
    const withTunnel = toConnectionInput(connectionSchema.parse({ ...valid, tunnelId: 't-1' }))
    expect(withTunnel.params).toMatchObject({ tunnelId: 't-1' })
  })

  it('validates sqlite on the path alone, ignoring server fields', () => {
    const sqlite = { ...valid, kind: 'sqlite', host: '', port: '', database: '', user: '', path: '/data/app.db' }
    const input = toConnectionInput(connectionSchema.parse(sqlite))
    expect(input.params).toEqual({ kind: 'sqlite', path: '/data/app.db' })
    expect(input.password).toBeNull()

    const missing = connectionSchema.safeParse({ ...sqlite, path: '  ' })
    expect(missing.success).toBe(false)
    if (!missing.success)
      expect(zodFieldErrors(missing.error).path).toBe('Database file is required')
  })

  it('trims the sqlite path', () => {
    const sqlite = { ...valid, kind: 'sqlite', path: ' /data/app.db ' }
    expect(toConnectionInput(connectionSchema.parse(sqlite)).params).toMatchObject({ path: '/data/app.db' })
  })

  it('maps the credential mode to the tagged core shape', () => {
    expect(toConnectionInput(connectionSchema.parse(valid)).credential).toEqual({ mode: 'keychain' })
    expect(toConnectionInput(connectionSchema.parse({ ...valid, credentialMode: 'prompt' })).credential)
      .toEqual({ mode: 'prompt' })

    const command = { ...valid, credentialMode: 'command', credentialCommand: '  aws rds token {host} ' }
    expect(toConnectionInput(connectionSchema.parse(command)).credential)
      .toEqual({ mode: 'command', command: 'aws rds token {host}', refreshAfterSecs: null })
  })

  it('requires a command in command mode', () => {
    const result = connectionSchema.safeParse({ ...valid, credentialMode: 'command', credentialCommand: '  ' })
    expect(result.success).toBe(false)
    if (!result.success)
      expect(zodFieldErrors(result.error).credentialCommand).toBe('Command is required')
  })

  it('keeps sqlite on the keychain mode whatever the form holds', () => {
    const sqlite = { ...valid, kind: 'sqlite', path: '/data/app.db', credentialMode: 'command', credentialCommand: 'x' }
    expect(toConnectionInput(connectionSchema.parse(sqlite)).credential).toEqual({ mode: 'keychain' })
  })
})

describe('sqlite profiles', () => {
  const stored: ConnectionProfile = {
    id: 'c-2',
    name: 'local file',
    env: 'dev',
    group: null,
    params: { kind: 'sqlite', path: '/data/app.db' },
  }

  it('roundtrips a sqlite profile through the form', () => {
    const values = formValuesFromProfile(stored)
    expect(values.kind).toBe('sqlite')
    expect(values.path).toBe('/data/app.db')
    const input = toConnectionInput(connectionSchema.parse(values))
    expect(input.params).toEqual(stored.params)
  })

  it('renders file-based identity lines', () => {
    expect(connectionTarget(stored.params)).toBe('/data/app.db')
    expect(connectionDsn(stored.params)).toBe('sqlite:///data/app.db')
    expect(connectionTarget(profile('p', null).params)).toBe('h:5432/db')
    expect(connectionDsn(profile('p', null).params)).toBe('postgres://u@h:5432/db')
  })

  it('badges the embedded engine version', () => {
    expect(serverBadge('sqlite', '3.50.1')).toEqual({ engine: 'SQLite', version: '3.50.1' })
  })

  it('restores the next default port when leaving sqlite', () => {
    // Into sqlite: the (hidden) port keeps its value.
    expect(portForKindChange(5432, 'postgres', 'sqlite')).toBe(5432)
    // Out of sqlite: whatever was left behind never counts as hand-set.
    expect(portForKindChange(0, 'sqlite', 'mysql')).toBe(3306)
    expect(portForKindChange(5432, 'sqlite', 'postgres')).toBe(5432)
  })
})

describe('redis profiles', () => {
  const stored: ConnectionProfile = {
    id: 'c-3',
    name: 'cache',
    env: 'dev',
    group: null,
    params: { kind: 'redis', host: 'cache.internal', port: 6380, db: 2, username: 'app', tls: true, tunnelId: 't-1' },
  }

  it('validates on host and port alone', () => {
    const values = { ...formValuesFromProfile(stored), password: 's3cret' }
    const input = toConnectionInput(connectionSchema.parse(values))
    expect(input.params).toEqual(stored.params)
    expect(input.password).toBe('s3cret')

    const noUser = connectionSchema.safeParse({ ...values, user: '' })
    expect(noUser.success).toBe(true)
    const noHost = connectionSchema.safeParse({ ...values, host: '' })
    expect(noHost.success).toBe(false)
    if (!noHost.success)
      expect(zodFieldErrors(noHost.error).host).toBe('Host is required')
  })

  it('turns a blank user into a null acl username', () => {
    const values = { ...formValuesFromProfile(stored), user: ' ' }
    expect(toConnectionInput(connectionSchema.parse(values)).params).toMatchObject({ username: null })
  })

  it('parses redis:// urls with the db index in the path', () => {
    expect(parseConnectionUrl('redis://app:pw@cache.internal:6380/2')).toMatchObject({
      kind: 'redis',
      host: 'cache.internal',
      port: 6380,
      dbIndex: 2,
      user: 'app',
      password: 'pw',
      tls: false,
      database: '',
    })
    expect(parseConnectionUrl('rediss://u@h/0')).toMatchObject({ tls: true, dbIndex: 0 })
    expect(parseConnectionUrl('redis://h')).toMatchObject({ port: 6379, dbIndex: 0 })
  })

  it('renders identity lines and badges, valkey included', () => {
    expect(connectionTarget(stored.params)).toBe('cache.internal:6380/2')
    expect(connectionDsn(stored.params)).toBe('rediss://cache.internal:6380/2')
    expect(serverBadge('redis', '7.4.1')).toEqual({ engine: 'Redis', version: '7.4.1' })
    expect(serverBadge('redis', '8.0.1-valkey')).toEqual({ engine: 'Valkey', version: '8.0.1' })
  })
})

describe('mongo profiles', () => {
  const stored: ConnectionProfile = {
    id: 'c-4',
    name: 'docs',
    env: 'dev',
    group: null,
    params: { kind: 'mongo', host: 'db.internal', port: 27018, database: 'app', username: 'reader', authSource: 'admin', tls: true, tunnelId: 't-1' },
  }

  it('round-trips through the form values, authSource included', () => {
    const values = { ...formValuesFromProfile(stored), password: 's3cret' }
    const input = toConnectionInput(connectionSchema.parse(values))
    expect(input.params).toEqual(stored.params)
    expect(input.password).toBe('s3cret')
  })

  it('validates on host and port alone', () => {
    const values = formValuesFromProfile(stored)
    const bare = connectionSchema.safeParse({ ...values, database: '', user: '', authSource: '' })
    expect(bare.success).toBe(true)
    const noHost = connectionSchema.safeParse({ ...values, host: '' })
    expect(noHost.success).toBe(false)
    if (!noHost.success)
      expect(zodFieldErrors(noHost.error).host).toBe('Host is required')
  })

  it('turns blank optionals into nulls', () => {
    const values = { ...formValuesFromProfile(stored), database: ' ', user: ' ', authSource: ' ' }
    expect(toConnectionInput(connectionSchema.parse(values)).params)
      .toMatchObject({ database: null, username: null, authSource: null })
  })

  it('parses mongodb:// urls with authSource and tls params', () => {
    expect(parseConnectionUrl('mongodb://reader:pw@db.internal:27018/app?authSource=admin&tls=true')).toMatchObject({
      kind: 'mongo',
      host: 'db.internal',
      port: 27018,
      database: 'app',
      user: 'reader',
      password: 'pw',
      authSource: 'admin',
      tls: true,
    })
    expect(parseConnectionUrl('mongodb://h')).toMatchObject({ kind: 'mongo', port: 27017, tls: false })
    expect(parseConnectionUrl('mongodb://h/app')).not.toHaveProperty('authSource')
  })

  it('renders identity lines and the badge', () => {
    expect(connectionTarget(stored.params)).toBe('db.internal:27018/app')
    expect(connectionDsn(stored.params)).toBe('mongodb://reader@db.internal:27018/app')
    expect(connectionTarget({ kind: 'mongo', host: 'h', port: 27017 })).toBe('h:27017')
    expect(serverBadge('mongo', '8.0.4')).toEqual({ engine: 'Mongo', version: '8.0.4' })
  })
})
