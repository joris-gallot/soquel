#!/usr/bin/env bash
# Runs the Rust integration suites against the compose databases (pnpm db:test).
# Port plan and version floors: docker-compose.test.yml.
set -euo pipefail
cd "$(dirname "$0")/.."

manifest=src-tauri/Cargo.toml

echo "==> full suite (current versions)"
SOQUEL_TEST_PG=postgres://soquel:soquel@localhost:5455/soquel_test \
SOQUEL_TEST_PG_TLS=postgres://soquel:soquel@localhost:5459/soquel_test \
SOQUEL_TEST_MYSQL=localhost:5456 \
SOQUEL_TEST_REDIS=localhost:5457 \
SOQUEL_TEST_SSH=localhost:5458 \
SOQUEL_TEST_SSH_RECONNECT=localhost:5461 \
  cargo test --manifest-path "$manifest" integration_

echo "==> postgres oldest supported"
SOQUEL_TEST_PG=postgres://soquel:soquel@localhost:5460/soquel_test \
  cargo test --manifest-path "$manifest" integration_postgres_

echo "==> mysql oldest supported"
SOQUEL_TEST_MYSQL=localhost:5462 \
  cargo test --manifest-path "$manifest" integration_mysql_

echo "==> mariadb (mysql kind)"
SOQUEL_TEST_MYSQL=localhost:5463 \
  cargo test --manifest-path "$manifest" integration_mysql_
