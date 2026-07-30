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
