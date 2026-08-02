export const SITE = {
  name: 'soquel',
  url: 'https://soquel.dev',
  repo: 'https://github.com/joris-gallot/soquel',
  author: 'Joris Gallot',
  licence: 'https://spdx.org/licenses/FSL-1.1-MIT',
  tagline: 'a database client that lends agents the query, not the credentials',
  description:
    'A desktop database client for Postgres, MySQL, SQLite, Redis and MongoDB. Coding agents reach your data through the app over MCP: read-only by default, every write stops for your approval, and credentials never leave the Rust core.',
} as const

export const ENGINES = [
  { name: 'PostgreSQL', note: '14 and up' },
  { name: 'MySQL, MariaDB', note: 'MySQL 8.0 and up, MariaDB LTS' },
  { name: 'SQLite', note: 'a file, no server' },
  { name: 'Redis, Valkey', note: 'key browser and console' },
  { name: 'MongoDB', note: 'document browser and console' },
] as const

export const FEATURES = [
  'Table browser with inline editing, filters and export',
  'SQL editor with query plans, rendered as a tree',
  'SSH tunnels with key, agent or password auth, and host key verification',
  'TLS, custom root certificates included',
  'Passwords from the OS keychain, asked at connect, or read from a command',
  'Connections exported to a file, passwords left out unless you encrypt them in',
] as const
