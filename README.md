# soquel

Next generation database client.

Tauri 2 desktop app: Rust core (drivers, SSH tunnels, streaming) + Vue 3 webview. Postgres first, MySQL and Redis later.

## Development

```bash
pnpm install
pnpm dev        # tauri dev (Rust core + vite)
pnpm dev:app    # vite only, webview in a browser
```

See `AGENTS.md` for the full command list and architecture rules.

## License

[FSL-1.1-MIT](LICENSE). Source available: use it, read it, build it for yourself.
