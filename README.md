# soquel

Next generation database client.

A Tauri 2 desktop app: the Rust core owns everything heavy and sensitive (database drivers, SSH tunnels, connection pools, result streaming, credentials), the Vue 3 webview is a thin client on top. Pre-release, under active development: no builds to download yet.

## Databases

| Engine | Notes |
| --- | --- |
| PostgreSQL | 14+ |
| MySQL / MariaDB | MySQL 8.0+, MariaDB LTS |
| SQLite | file-based, no server needed |
| Redis / Valkey | key browser + console |
| MongoDB | document browser + console |

## Features

- Table browser with inline editing, filters and export
- SQL editor with query plans (EXPLAIN tree)
- SSH tunnels: key and agent auth, host key verification
- TLS connections, including custom root certificates
- Redis key browser, Mongo document browser, dedicated consoles
- Command palette

## Design

- **Secrets never reach the webview.** Credentials live in the Rust core and the OS keychain.
- **A typed command layer is the only IPC boundary.** Every operation is a Tauri command with a normalized error shape; TypeScript bindings are generated from the Rust types.
- **Offline by design.** No remote assets, strict CSP; the app never phones home.

On the roadmap: a local MCP server mode exposing your connections to agents through that same command layer, with UI guardrails (read-only by default, visual approval for writes).

## Development

Prerequisites: Rust (stable), Node + pnpm, and the [Tauri v2 system dependencies](https://tauri.app/start/prerequisites/) on Linux.

```bash
pnpm install
pnpm dev        # tauri dev (Rust core + vite)
pnpm dev:app    # vite only, webview in a browser

pnpm db:dev     # local dev databases (docker compose), see AGENTS.md for seeds
pnpm db:test    # throwaway seeded databases for the test suites
pnpm test:integration
```

See `AGENTS.md` for the full command list and architecture rules.

## License

[FSL-1.1-MIT](LICENSE). Source available: use it, read it, build it for yourself.
