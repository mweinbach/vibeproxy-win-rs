# CodeForwarder (macOS + Windows + Linux)

CodeForwarder is a desktop app (macOS + Windows + Linux) that runs a local OAuth/authentication proxy and unified model router on `http://localhost:8317`.

It’s built with **Tauri 2 (Rust)** + **React 19 / Vite / TypeScript**, and bundles (or downloads) the **CLIProxyAPIPlus** runtime used to perform provider logins and serve the backend API.

## What it does

- Starts/stops a local proxy stack:
  - `127.0.0.1:8317` = **ThinkingProxy** (this app)
  - `127.0.0.1:8318` = **CLIProxyAPIPlus** backend (`cli-proxy-api-plus` / `cli-proxy-api-plus.exe`)
- Manages provider accounts (OAuth tokens stored locally in `~/.cli-proxy-api/`)
- Enables “thinking” requests for Claude models by interpreting model suffixes like `-thinking-5000`
- Optional Claude routing via **Vercel AI Gateway** (API key stored encrypted)
- System tray controls + launch at login
- Built-in local usage analytics dashboard (requests/tokens by provider/model/account)

## Prerequisites

- macOS (Apple Silicon or Intel), Windows 10/11, or Ubuntu/Linux
- **Bun** (used by `src-tauri/tauri.conf.json` for dev/build orchestration)
- **Node.js 18+** (required for `scripts/sync-cli-proxy-binary.mjs` because it uses `fetch`)
- Rust toolchain (stable) + Tauri prerequisites
  - Ubuntu: install Tauri Linux deps (system webview and tray deps). Typical set:
    - `pkg-config`, `libglib2.0-dev`, `libgtk-3-dev`, `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `build-essential`

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
  - CodeForwarder also writes `merged-config.yaml` here when providers are toggled or Z.AI keys are added.
- Downloaded runtime binary: `%LOCALAPPDATA%\codeforwarder\cli-proxy-api-plus.exe`
- Downloaded runtime binary (macOS): `~/Library/Application Support/codeforwarder/cli-proxy-api-plus`
- Downloaded runtime binary (Linux): `~/.local/share/codeforwarder/cli-proxy-api-plus`
- App settings: stored via Tauri Store (`settings.json`), with the Vercel API key encrypted using Windows DPAPI (base64 fallback on non-Windows).
- Usage analytics DB: `~/.cli-proxy-api/codeforwarder-usage.db` (local-only)

## Updating the bundled runtime

To refresh the bundled runtime (`src-tauri/resources/cli-proxy-api-plus` / `src-tauri/resources/cli-proxy-api-plus.exe`) from the latest GitHub release:

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

## Releases & Auto-Updates

- The app uses the Tauri updater plugin and checks GitHub Releases metadata at:
  - `https://github.com/mweinbach/CodeForwarder/releases/latest/download/latest.json`
- Release builds (including updater signatures and macOS notarization) run automatically when pushing a tag like `v0.1.4`.
- `scripts/release.mjs` bumps versions, creates the tag and GitHub release; the CI tag trigger does the rest.
