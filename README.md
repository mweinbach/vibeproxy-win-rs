# VibeProxy (Windows)

VibeProxy is a Windows desktop app that runs a local OAuth/authentication proxy and unified model router on `http://localhost:8317`.

It’s built with **Tauri 2 (Rust)** + **React 19 / Vite / TypeScript**, and bundles (or downloads) the **CLIProxyAPIPlus** runtime used to perform provider logins and serve the backend API.

## What it does

- Starts/stops a local proxy stack:
  - `127.0.0.1:8317` = **ThinkingProxy** (this app)
  - `127.0.0.1:8318` = **CLIProxyAPIPlus** backend (`cli-proxy-api-plus.exe`)
- Manages provider accounts (OAuth tokens stored locally in `~/.cli-proxy-api/`)
- Enables “thinking” requests for Claude models by interpreting model suffixes like `-thinking-5000`
- Optional Claude routing via **Vercel AI Gateway** (API key stored encrypted)
- System tray controls + “launch at login”

## Prerequisites

- Windows 10/11
- **Bun** (used by `src-tauri/tauri.conf.json` for dev/build orchestration)
- **Node.js 18+** (required for `scripts/sync-cli-proxy-binary.mjs` because it uses `fetch`)
- Rust toolchain (stable) + Tauri prerequisites

## Quick start (development)

```bash
bun install
bun run tauri dev
```

Frontend-only (runs Vite; Tauri `invoke()` calls are safely ignored):

```bash
bun run dev
```

## Build

Web build only:

```bash
bun run build
```

Desktop build:

```bash
bun run tauri build
```

`tauri build` runs `bun run sync:cli-proxy-binary && bun run build` first (see `src-tauri/tauri.conf.json`).

## Using the proxy

Point your client/tooling at:

- Base URL: `http://localhost:8317`

## Local files & storage

- Auth/account files: `~/.cli-proxy-api/` (JSON)
  - VibeProxy also writes `merged-config.yaml` here when providers are toggled or Z.AI keys are added.
- Downloaded runtime binary: `%LOCALAPPDATA%\vibeproxy\cli-proxy-api-plus.exe`
- App settings: stored via Tauri Store (`settings.json`), with the Vercel API key encrypted using Windows DPAPI.

## Updating the bundled runtime

To refresh `src-tauri/resources/cli-proxy-api-plus.exe` from the latest GitHub release:

```bash
bun run sync:cli-proxy-binary
```

To skip this step (CI/offline builds), set `SKIP_CLI_PROXY_SYNC=1`.

## Notes

- Closing the main window hides the app to the system tray (use the tray icon to reopen, or quit).
- Ports `8317` and `8318` must be available.

## Documentation

- [Development](docs/DEVELOPMENT.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Troubleshooting](docs/TROUBLESHOOTING.md)
