# Development Guide

## Tooling

- Frontend: React 19 + Vite 7 + TypeScript
- Desktop shell/backend: Tauri 2 + Rust
- Package manager in this repo: Bun

## Commands

```bash
bun install
bun run dev
bun run build
bun run tauri dev
bun run tauri build
bun run sync:cli-proxy-binary
```

Note: `bun run tauri dev` and `bun run tauri build` automatically run `sync:cli-proxy-binary` first via `src-tauri/tauri.conf.json`.

## Runtime/binary behavior

- `src-tauri/resources/cli-proxy-api-plus` (macOS/Linux) and `src-tauri/resources/cli-proxy-api-plus.exe` (Windows) are the bundled runtimes used in packaged builds.
- On startup, backend code resolves a runnable binary in this order:
1. Platform data dir:
   - Windows: `%LOCALAPPDATA%\vibeproxy\cli-proxy-api-plus.exe`
   - macOS: `~/Library/Application Support/vibeproxy/cli-proxy-api-plus`
2. bundled resource binary (`src-tauri/resources/cli-proxy-api-plus*`)
 - If bundled exists but local copy does not, it is copied into the platform data directory when possible.

## Dev vs web-only mode

- `bun run tauri dev` runs full app behavior (Rust commands + tray + local proxy).
- `bun run dev` runs only the Vite frontend; Tauri `invoke()` calls are caught and ignored in places where web mode is supported.

## Persisted app data

- Auth accounts directory: `~/.cli-proxy-api/`
- Merged config output: `~/.cli-proxy-api/merged-config.yaml`
- Settings store: Tauri Store `settings.json`
- Usage analytics DB: `~/.cli-proxy-api/vibeproxy-usage.db`
- Sensitive values:
  - `vercel_api_key` in settings is encrypted via DPAPI (`secure_store.rs`)
  - Z.AI keys are stored in `~/.cli-proxy-api/zai-*.json` with encrypted `api_key`
  - Managed remote-management key is stored locally encrypted (used for internal native usage reads)

## Usage analytics notes

- The Usage tab is backed by first-party local tracking in `thinking_proxy.rs` and `usage_tracker.rs`.
- Inference request events are tracked for `/v1`, `/api/v1`, and `/api/provider` paths.
- Native usage comparison is temporary and best-effort (`usage_native.rs`).
- Native comparison "all-time" view is clamped to 30d and labeled in the UI.

## Important implementation notes

- The window is frameless (`decorations: false`) and uses a custom title bar (`TitleBar.tsx`).
- Closing the window hides to tray; exit via tray menu `Quit`.
- App startup auto-starts proxy only when runtime binary is available.
- Provider enable/disable settings are merged into generated YAML using `config_manager.rs`.
