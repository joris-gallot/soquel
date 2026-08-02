import type { APIRoute } from 'astro'
import { ENGINES, FEATURES, SITE } from '@/lib/site'

/// The long form: every claim the page makes, with the detail behind it.
export const GET: APIRoute = () => {
  const engines = ENGINES.map(engine => `- ${engine.name}: ${engine.note}`).join('\n')
  const features = FEATURES.map(feature => `- ${feature}`).join('\n')

  const body = `# ${SITE.name}

> ${SITE.description}

Pre-release, under active development. The source is available under FSL-1.1-MIT: read it, build it for yourself. No signed builds are published yet.

## Shape

A Tauri 2 desktop app. The Rust core owns everything heavy and sensitive: database drivers, SSH tunnels, connection pools, result streaming and credentials. The Vue 3 webview is a thin client on top of a typed command layer, which is the only boundary between the two.

## Engines

${engines}

## What the client does

${features}

## Agent access over MCP

Soquel can run a local MCP server so coding agents query databases through the app instead of getting a copy of the credentials. It is turned on from the connections screen, and the app prints the command to paste into the agent.

The guardrails are the point:

- Off, and empty, by default. The server starts stopped, and every connection is invisible to agents until opted in. Read-only or writes-need-approval, per connection.
- Read-only is the engine's job, not a SQL parser's. Agent reads run inside a READ ONLY transaction on Postgres and MySQL, or on a handle opened read-only at the filesystem level on SQLite. A statement that slips past classification still cannot write.
- Writes stop for a human. A write opens a dialog showing the exact statement. Denying it, closing it, or ignoring it for a minute all refuse.
- Every call is logged: tool, connection, statement, outcome and duration, readable from the app.
- Bounded. Results are capped and paginated, and agent queries carry a 30 second engine-enforced timeout so a runaway query cannot camp on a connection.
- Local only. The server binds loopback and requires a bearer token that never leaves the machine.

Agents get read tools for every supported engine: schema and DDL, SQL queries, table samples, Redis keys, Mongo documents and indexes. No tool mutates anything except the one that asks permission first.

## Credentials

A connection's password comes from the OS keychain, from a prompt at connect time (kept in memory only), or from a command whose stdout is the password, which covers RDS IAM tokens, Vault and 1Password. The same three modes apply to SSH passwords and key passphrases.

An imported connections file cannot run code on its own: a credential command that arrived through an import stays inert until its exact arguments have been read and approved. Exports leave passwords out unless they are explicitly included, and then the file is encrypted.

An OS keyring is required. Without one, the keychain mode is unavailable and the app says so; the prompt and command modes still work.

## Design commitments

- Secrets never reach the webview. They live in the Rust core and the OS keychain.
- A typed command layer is the only IPC boundary. Every operation is a Tauri command with a normalised error shape, and the TypeScript bindings are generated from the Rust types. The MCP tools are that same layer with a second client.
- Offline by design: no remote assets, a strict CSP, and the app does not phone home. Updates are checked only when asked or at startup, against a single endpoint.

## Links

- Source: ${SITE.repo}
- Licence: FSL-1.1-MIT
`

  return new Response(body, { headers: { 'Content-Type': 'text/plain; charset=utf-8' } })
}
