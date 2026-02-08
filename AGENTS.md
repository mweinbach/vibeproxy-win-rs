# Repository Guidelines

## Project Structure & Module Organization
- `src/`: React 19 + TypeScript frontend (UI components, hooks, shared types).
- `src-tauri/src/`: Rust backend for Tauri commands, proxy logic, process/binary management, and tray behavior.
- `src-tauri/resources/`: bundled runtime/config assets (including `cli-proxy-api-plus` / `cli-proxy-api-plus.exe` for packaged builds).
- `scripts/`: build/support scripts such as runtime sync (`sync-cli-proxy-binary.mjs`).
- `docs/`: architecture, development workflow, and troubleshooting notes.
- Build artifacts (`dist/`, `src-tauri/target/`, `node_modules/`) are generated; avoid editing them directly.

## Build, Test, and Development Commands
- `bun install`: install JS dependencies.
- `bun run dev`: run frontend-only Vite app (`http://localhost:1420`).
- `bun run tauri dev`: run full desktop app (frontend + Rust backend).
- `bun run build`: TypeScript check + Vite production build.
- `bun run tauri build`: desktop production build (runs binary sync + web build first).
- `bun run sync:cli-proxy-binary`: fetch latest platform runtime into `src-tauri/resources/` (defaults to host arch; override with `CLI_PROXY_TARGET_ARCH=amd64|arm64` when cross-building).
- `cargo test --manifest-path src-tauri/Cargo.toml`: run Rust unit tests.

## Release & Remote Builds
- Keep versions in sync across `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml`.
- When cutting a new version/tag (`vX.Y.Z`), trigger remote builds for that exact ref and (optionally) upload the artifacts to the matching GitHub release tag:
  - macOS builds: `aarch64-apple-darwin` and `x86_64-apple-darwin` DMGs
  - Windows builds: `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` installers (NSIS + MSI)
- Example (requires the GitHub release tag to already exist):
  - `gh workflow run "Build macOS (Tauri)" -f ref=vX.Y.Z -f release_tag=vX.Y.Z`
  - `gh workflow run "Build Windows (Tauri)" -f ref=vX.Y.Z -f release_tag=vX.Y.Z`

## Coding Style & Naming Conventions
- Frontend: 2-space indentation, semicolons, double quotes, PascalCase component files (`SettingsView.tsx`), camelCase functions/hooks (`useServerState`), and colocated hooks under `src/hooks/`.
- Backend: standard Rust style (rustfmt defaults), snake_case modules/files (`server_manager.rs`), descriptive function names.
- Keep provider/model constants centralized in `src/types/` when possible.

## Testing Guidelines
- Rust unit tests exist inline with backend modules (notably `thinking_proxy.rs` and `server_manager.rs`).
- Add tests when touching proxy request rewriting, model transformation, process lifecycle, or config merging.
- No frontend test framework is currently configured; if adding one, keep test files near the feature (`*.test.ts[x]`).
- There is no formal coverage gate; focus on meaningful behavior coverage.

## Commit & Pull Request Guidelines
- Current history uses short, lowercase subjects (for example: `fix the sizing`, `updates`).
- Prefer concise imperative subjects; include scope when helpful (example: `settings: handle missing binary state`).
- PRs should include: summary, affected areas (`src/` vs `src-tauri/`), verification steps/commands, and screenshots for UI changes.
- Link related issues/tasks and note any config or runtime-binary impacts.
- Every change should be accompanied by a corresponding test case.
- Every change should also be committed once it is complete. Feel free to commit liberally.

## Security & Configuration Tips
- Treat secrets and auth data as sensitive (`~/.cli-proxy-api/`, local settings store).
- Use `SKIP_CLI_PROXY_SYNC=1` only for offline/CI scenarios where runtime sync must be skipped.
