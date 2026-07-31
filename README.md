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
- Agent access over MCP, off by default (see below)

## Agent access (MCP)

Soquel can run a local MCP server so coding agents query your databases through the app instead of getting a copy of your credentials. Turn it on from the connections screen and paste the generated command into your agent:

```bash
claude mcp add --transport http soquel http://127.0.0.1:52700/mcp --header "Authorization: Bearer <token>"
```

The guardrails are the point:

- **Off, and empty, by default.** The server starts stopped, and every connection is invisible to agents until you opt it in. Read-only or writes-need-approval, per connection.
- **Read-only is the engine's job, not a SQL parser's.** Agent reads run inside a `READ ONLY` transaction (Postgres, MySQL/MariaDB) or on a handle opened read-only at the filesystem level (SQLite). A statement that slips past classification still cannot write.
- **Writes stop for a human.** A write opens a dialog showing the exact statement. Denying, closing it, or ignoring it for a minute all refuse.
- **Every call is logged.** Tool, connection, statement, outcome and duration, readable from the app.
- **Bounded.** Results are capped and paginated; agent queries carry a 30s engine-enforced timeout so a runaway query cannot camp on a connection.
- **Local only.** The server binds loopback and requires a bearer token that never leaves your machine.

Agents get read tools for every supported engine (schema and DDL, SQL queries, table samples, Redis keys, Mongo documents and indexes). There is no tool that mutates anything except the one that asks permission first.

## Design

- **Secrets never reach the webview.** Credentials live in the Rust core and the OS keychain.
- **A typed command layer is the only IPC boundary.** Every operation is a Tauri command with a normalized error shape; TypeScript bindings are generated from the Rust types. The MCP tools above are that same layer with a second client.
- **Offline by design.** No remote assets, strict CSP; the app never phones home.

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
