// Databases from docker-compose.test.yml (`pnpm db:test`), one entry per connector kind.
export const TEST_DBS = {
  postgres: {
    host: 'localhost',
    port: '5455',
    database: 'soquel_test',
    user: 'soquel',
    password: 'soquel',
  },
  mysql: {
    host: 'localhost',
    port: '5456',
    database: 'soquel_test',
    user: 'soquel',
    password: 'soquel',
  },
} as const

export const TEST_REDIS = {
  host: 'localhost',
  port: '5457',
  password: 'soquel',
} as const

// The e2e spec browses the seeded soquel_e2e db (scripts/test-seeds/mongo.js).
export const TEST_MONGO = {
  host: 'localhost',
  port: '5464',
  user: 'soquel',
  password: 'soquel',
} as const
