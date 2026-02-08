# Architecture

## High-level flow

VibeProxy runs two local servers:

- `127.0.0.1:8317` - **ThinkingProxy** (Rust, in-process HTTP proxy)
- `127.0.0.1:8318` - **CLIProxyAPIPlus** (external process: `cli-proxy-api-plus` / `cli-proxy-api-plus.exe`)

Client tools should talk to **`http://localhost:8317`**.

```text
Client / SDKs / CLI tools
        |
        v
  http://127.0.0.1:8317   (ThinkingProxy)
        |
        v
  http://127.0.0.1:8318   (CLIProxyAPIPlus runtime)
```

## Frontend (React)

The UI lives under `src/` and uses Tauri `invoke()` commands to control the backend:

- `useServerState` -> `get_server_state`, `start_server`, `stop_server`, `download_binary`
- `useAuthAccounts` -> `get_auth_accounts`, `run_auth`, `delete_auth_account`, `save_zai_api_key`
- `useSettings` -> `get_settings`, `set_provider_enabled`, `set_vercel_config`, `set_launch_at_login`
- `useUsageDashboard` -> `get_usage_dashboard`

The app is a single view (`SettingsView`) with 4 tabs:

- Dashboard (runtime status + start/stop)
- Usage (request/token analytics + native comparison panel)
- Services (connect accounts + enable/disable providers)
- Settings (launch at login, open auth folder)

## Backend (Tauri/Rust)

Core modules in `src-tauri/src/`:

- `commands.rs` - Tauri command handlers exposed to the UI
- `thinking_proxy.rs` - local HTTP proxy on `8317`
- `server_manager.rs` - process manager for `cli-proxy-api-plus` / `cli-proxy-api-plus.exe` (spawn/stop/auth helpers)
- `binary_manager.rs` - resolves bundled vs downloaded runtime; downloads latest release and verifies SHA-256
- `auth_manager.rs` - scans/deletes auth JSON files in `~/.cli-proxy-api/`
- `config_manager.rs` - merges base config with provider toggles + Z.AI keys + managed remote-management key
- `settings.rs` + `secure_store.rs` - settings persistence with DPAPI encryption for secrets (base64 fallback on non-Windows)
- `usage_tracker.rs` - local SQLite usage storage and dashboard aggregation
- `usage_native.rs` - temporary native usage comparison fetch/parsing
- `managed_key.rs` - generation/storage of internal management key for local-only native usage reads
- `tray.rs` - system tray menu + themed icons; window close hides to tray

## ThinkingProxy request handling

`ThinkingProxy` is a lightweight HTTP/1 proxy with a few special behaviors:

1. **Amp CLI login redirect support**
   - `/auth/cli-login` and `/api/auth/cli-login` are redirected to `https://ampcode.com/...`.

2. **Amp provider path rewriting**
   - Requests to `/provider/...` are rewritten to `/api/provider/...`.

3. **Amp management requests**
   - Any request that is *not* targeting `/v1/...` or `/api/provider/...` is forwarded to `https://ampcode.com`.

4. **Claude thinking support**
   - For POST bodies with Claude models suffixed like `-thinking-<budget>`:
     - strips the suffix from `model`
     - injects a JSON `thinking` object
     - bumps `max_tokens` / `max_output_tokens` to be above the budget
     - adds the `anthropic-beta: interleaved-thinking-2025-05-14` header

5. **Optional Vercel AI Gateway routing**
   - If enabled and a Vercel key is configured, Claude requests can be routed to `https://ai-gateway.vercel.sh/v1/messages`.

6. **Usage tracking**
   - Inference requests (`/v1`, `/api/v1`, `/api/provider`) are tracked in local SQLite.
   - Captures request count, status, provider/model/account attribution, and token usage (input/output/total/cached/reasoning) when available.

## Config merging

Base config ships at `src-tauri/resources/config.yaml`.

VibeProxy writes merged config to:

- `~/.cli-proxy-api/merged-config.yaml`

Changes applied by the merger:

- `oauth-excluded-models`: adds provider keys marked disabled in UI
- `openai-compatibility`: injects `zai` endpoint + API key entries and common GLM model aliases
- `remote-management.secret-key`: generated and managed by VibeProxy for internal local native usage reads
- `remote-management.allow-remote`: forced to `false`

## Usage analytics data

- Local database path: `~/.cli-proxy-api/vibeproxy-usage.db`
- VibeProxy-tracked events are first-party and local-only.
- Native comparison data is temporary, best-effort, and shown side-by-side in the Usage tab.

## Security model (practical)

- Servers bind to `127.0.0.1` only.
- Remote management stays localhost-only (`allow-remote: false`).
- Secrets are encrypted at rest on Windows using DPAPI (per-user), with a base64 fallback on non-Windows. Treat local files as sensitive anyway.
