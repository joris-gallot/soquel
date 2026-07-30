import type { ConnectionProfile } from '@/lib/bindings'
import { describe, expect, it } from 'vitest'
import { connectionSchema, formValuesFromProfile, groupConnections, parseConnectionUrl, portForKindChange, serverBadge, toConnectionInput } from './connections'
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
    expect(parseConnectionUrl('redis://u:p@h:6379/0')).toBeNull()
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

describe('connectionSchema', () => {
  const valid = {
    name: 'local',
    env: 'dev',
    kind: 'postgres',
    host: 'localhost',
    port: '5432',
    database: 'app',
    user: 'postgres',
    sslMode: 'prefer',
    sslRootCert: '',
    tunnelId: '',
    group: '',
    password: '',
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
    expect(toConnectionInput(connectionSchema.parse(verifyFull)).params.sslRootCert).toBe('/etc/ca.pem')
    // Any other mode drops the stale path instead of persisting it.
    const require = { ...valid, sslMode: 'require', sslRootCert: '/etc/ca.pem' }
    expect(toConnectionInput(connectionSchema.parse(require)).params.sslRootCert).toBeNull()
    expect(toConnectionInput(connectionSchema.parse(valid)).params.sslRootCert).toBeNull()
  })

  it('turns the no-tunnel sentinel into null for the command input', () => {
    expect(toConnectionInput(connectionSchema.parse(valid)).params.tunnelId).toBeNull()
    expect(toConnectionInput(connectionSchema.parse({ ...valid, tunnelId: 'none' })).params.tunnelId).toBeNull()
    // Reka Select can clear the model to undefined; the schema falls back to no tunnel.
    expect(toConnectionInput(connectionSchema.parse({ ...valid, tunnelId: undefined })).params.tunnelId).toBeNull()
    const withTunnel = toConnectionInput(connectionSchema.parse({ ...valid, tunnelId: 't-1' }))
    expect(withTunnel.params.tunnelId).toBe('t-1')
  })
})
