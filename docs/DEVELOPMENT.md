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

## Runtime/binary behavior

- `src-tauri/resources/cli-proxy-api-plus.exe` is the bundled runtime used in packaged builds.
- On startup, backend code resolves a runnable binary in this order:
  1. `%LOCALAPPDATA%\vibeproxy\cli-proxy-api-plus.exe`
  2. bundled resource binary (`src-tauri/resources/cli-proxy-api-plus.exe`)
- If bundled exists but local copy does not, it is copied into `%LOCALAPPDATA%\vibeproxy\` when possible.

## Dev vs web-only mode

- `bun run tauri dev` runs full app behavior (Rust commands + tray + local proxy).
- `bun run dev` runs only the Vite frontend; Tauri `invoke()` calls are caught and ignored in places where web mode is supported.

## Persisted app data

- Auth accounts directory: `~/.cli-proxy-api/`
- Merged config output: `~/.cli-proxy-api/merged-config.yaml`
- Settings store: Tauri Store `settings.json`
- Sensitive values:
  - `vercel_api_key` in settings is encrypted via DPAPI (`secure_store.rs`)
  - Z.AI keys are stored in `~/.cli-proxy-api/zai-*.json` with encrypted `api_key`

## Important implementation notes

- The window is frameless (`decorations: false`) and uses a custom title bar (`TitleBar.tsx`).
- Closing the window hides to tray; exit via tray menu `Quit`.
- App startup auto-starts proxy only when runtime binary is available.
- Provider enable/disable settings are merged into generated YAML using `config_manager.rs`.
