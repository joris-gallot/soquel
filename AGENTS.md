@~/.claude/stack/web-saas.md

# soquel

Guidance for agents in this repo. Shared stack conventions are imported above; everything below is soquel-specific and overrides the profile where they differ.

## What this is

Soquel is a next generation database client (TablePlus alternative): a Tauri 2 desktop app. The Rust core owns everything heavy and sensitive (DB drivers, SSH tunnels, connection pools, result streaming, credentials); the Vue webview is a thin client. v1 targets Postgres + SSH tunnels + table browser + SQL editor; MySQL and Redis come later behind a capability-based connector trait.

Two architecture rules that must hold:

- **Typed command layer is the only IPC boundary.** Every operation is a Tauri command with a normalized error shape. Commands are pure and independently callable: this surface later becomes the agent/MCP tool surface, the UI is just its first client.
- **Secrets never cross into the webview.** Credentials live in the Rust core and the OS keychain.

## Layout

- `packages/app` - Vue 3 + vue-router + shadcn-vue (Reka UI) + Tailwind v4, Vite on port 5173.
- `src-tauri` - Rust core. `src/commands.rs` holds the command layer, `src/error.rs` the normalized error enum.

### Command layer

Commands are annotated `#[tauri::command] #[specta::specta]`, return `Result<T, Error>`, and are registered in `specta_builder()` (`src-tauri/src/lib.rs`). TypeScript bindings are generated to `packages/app/src/lib/bindings.ts` (committed, eslint-ignored): automatically on `tauri dev`, or headless via `cargo test --manifest-path src-tauri/Cargo.toml export_typescript_bindings`. The frontend imports `commands` from the bindings and never calls `invoke` directly. Error handling is `ErrorHandlingMode::Result`: bindings return `{ status: 'ok', data } | { status: 'error', error: { kind, message } }`. New error variants go on the `Error` enum in `error.rs`, tagged by `kind`.

No server/backend package: this is a desktop app. Data-layer conventions from the profile (Hono/tRPC/Drizzle/knex) don't apply here.

## Commands

Run from the repo root.

```bash
pnpm install
pnpm dev:app       # vite only (webview in a browser, no Rust)
pnpm dev           # tauri dev: builds the Rust core, launches the app, serves vite
pnpm dev:wsl       # same with file-backed plaintext secrets (WSL has no OS keychain); dev only
pnpm build         # pnpm -r build (frontend)
pnpm build:desktop # tauri build (bundles the app)
pnpm typecheck     # vue-tsc across the workspace
pnpm lint          # eslint . (lint:fix to autofix)
pnpm test          # vitest across the workspace
pnpm test:e2e      # wdio drives the built debug binary via tauri-driver (Linux/Windows only)
                   # isolated app data (SOQUEL_DATA_DIR) + in-memory secrets (SOQUEL_EPHEMERAL_SECRETS)
                   # screenshots land in packages/app/e2e/screenshots/ (gitignored)

cargo check --manifest-path src-tauri/Cargo.toml   # fast Rust validation
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml

pnpm db:test           # start the test databases (docker-compose.test.yml), seeded + throwaway
pnpm test:integration  # cargo integration_* tests against them
pnpm db:test:down

pnpm db:dev            # dev postgres 5470 + mysql 5471 + redis 5472 + mongo 5473 (docker-compose.dev.yml), persistent volumes
                       # dev stays out of 5455-5464: that whole range belongs to docker-compose.test.yml
pnpm db:dev:seed:pg    # (re)seed the dev postgres: ~1.5M rows across a SaaS-shaped app schema
pnpm db:dev:seed:mysql # same shape for the dev mysql (soquel_dev)
pnpm db:dev:seed:redis # SaaS-shaped keys for the key browser (~16k: sessions, cache, queues, stream)
pnpm db:dev:seed:mongo # SaaS-shaped documents for the doc browser (~85k: users, orders, events, sessions)
pnpm db:dev:down
```

## Testing

Weight: Rust integration against real databases is the core; unit tests for pure logic; e2e stays a thin smoke layer.

- `docker-compose.test.yml`: one service per connector kind, seeded from `scripts/test-seeds/<engine>.sql`. Port plan: postgres 5455, mysql 5456, redis 5457, sshd tunnel target 5458, postgres-tls 5459 (self-signed cert from `scripts/test-tls/`, unseeded, TLS handshake tests only), postgres-oldest 5460, sshd-reconnect 5461, mysql-oldest 5462, mongo 5464 (seeds soquel_e2e for the e2e spec; integration tests own their soquel_test_* databases).
- Minimum supported postgres = oldest non-EOL major (currently 14). Minimum supported mysql = 8.0 (EOL upstream but dominant in the wild via RDS/Aurora extended support). The `*-oldest` services run them with the same seeds, and `pnpm test:integration` runs the `integration_postgres_*` / `integration_mysql_*` suites against both versions; the seeds must stay valid on the oldest.
- MariaDB is supported through the mysql kind (suite runs against `mariadb` LTS on 5463). Known quirks, asserted flavor-aware in tests: JSON columns read as text (LONGTEXT alias), COLUMN_TYPE keeps display widths (`int(11)`), KILL QUERY raises ER_QUERY_INTERRUPTED instead of returning; the workspace badge shows "MariaDB x.y.z" from the version string.
- Rust integration tests are named `integration_<engine>_*`, each gated by its env var (`SOQUEL_TEST_PG`, `SOQUEL_TEST_SSH`, later `SOQUEL_TEST_MYSQL`, ...) and skipped silently when unset. `pnpm test:integration` wires the env vars to the compose databases. SSH tunnel tests use the sshd service (key auth via the committed throwaway keypair in `scripts/test-ssh/`).
- e2e specs take DB coordinates from `packages/app/e2e/fixtures.ts` (never hardcode), and need `pnpm db:test` up.

## Tauri specifics

- Linux dev needs the Tauri v2 system prerequisites (webkit2gtk-4.1 etc.); `tauri dev` under WSL runs the Linux build via WSLg, not representative of Windows/macOS.
- Debug builds are isolated from releases: data dir gets a `/dev` subtree and the keychain service a `.dev` suffix, so `tauri dev` never touches an installed app's connections or secrets (`SOQUEL_DATA_DIR` still overrides everything for e2e).
- `tauri.conf.json`: dev server is vite on 5173 (strictPort); `beforeDevCommand`/`beforeBuildCommand` drive the app package through pnpm filters.
- Add Tauri plugins with `pnpm tauri add <name>`; their permissions go in `src-tauri/capabilities/default.json`.
- No remote assets in the webview (fonts, scripts): the app must work offline and keep a strict CSP. Bundle everything.

## UI

- shadcn-vue components via CLI from `packages/app`: `pnpm exec shadcn-vue add <name>` (check the registry before hand-rolling).
- Use the `frontend-design` skill for UI work; the app identity (theme, typography) is defined in `packages/app/src/style.css`.
