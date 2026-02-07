# Architecture

## High-level flow

VibeProxy runs two local servers:

- `127.0.0.1:8317` — **ThinkingProxy** (Rust, in-process HTTP proxy)
- `127.0.0.1:8318` — **CLIProxyAPIPlus** (external process: `cli-proxy-api-plus.exe`)

Client tools should talk to **`http://localhost:8317`**.

```
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

- `useServerState` → `get_server_state`, `start_server`, `stop_server`, `download_binary`
- `useAuthAccounts` → `get_auth_accounts`, `run_auth`, `delete_auth_account`, `save_zai_api_key`
- `useSettings` → `get_settings`, `set_provider_enabled`, `set_vercel_config`, `set_launch_at_login`

The app is a single view (`SettingsView`) with 3 tabs:

- Dashboard (runtime status + start/stop)
- Services (connect accounts + enable/disable providers)
- Settings (launch at login, open auth folder)

## Backend (Tauri/Rust)

Core modules in `src-tauri/src/`:

- `commands.rs` — Tauri command handlers exposed to the UI
- `thinking_proxy.rs` — local HTTP proxy on `8317`
- `server_manager.rs` — process manager for `cli-proxy-api-plus.exe` (spawn/stop/auth helpers)
- `binary_manager.rs` — resolves bundled vs downloaded runtime; downloads latest release and verifies SHA-256
- `auth_manager.rs` — scans/deletes auth JSON files in `~/.cli-proxy-api/`
- `config_manager.rs` — merges base config with provider toggles + Z.AI keys
- `settings.rs` + `secure_store.rs` — settings persistence with DPAPI encryption for secrets
- `tray.rs` — system tray menu + themed icons; window close hides to tray

## ThinkingProxy request handling

`ThinkingProxy` is a lightweight HTTP/1 proxy with a few special behaviors:

1. **Amp CLI login redirect support**
   - `/auth/cli-login` and `/api/auth/cli-login` are redirected to `https://ampcode.com/...`.

2. **Amp provider path rewriting**
   - Requests to `/provider/...` are rewritten to `/api/provider/...`.

3. **Amp management requests**
   - Any request that is *not* targeting `/v1/...` or `/api/provider/...` is forwarded to `https://ampcode.com`.

4. **Claude “thinking” support**
   - For POST bodies with Claude models suffixed like `-thinking-<budget>`:
     - strips the suffix from `model`
     - injects a JSON `thinking` object
     - bumps `max_tokens` / `max_output_tokens` to be above the budget
     - adds the `anthropic-beta: interleaved-thinking-2025-05-14` header

5. **Optional Vercel AI Gateway routing**
   - If enabled and a Vercel key is configured, Claude requests can be routed to `https://ai-gateway.vercel.sh/v1/messages`.

## Config merging

Base config ships at `src-tauri/resources/config.yaml`.

When providers are disabled or Z.AI keys exist, VibeProxy writes:

- `~/.cli-proxy-api/merged-config.yaml`

Changes applied by the merger:

- `oauth-excluded-models`: adds provider keys marked disabled in UI
- `openai-compatibility`: injects `zai` endpoint + API key entries and common GLM model aliases

## Security model (practical)

- Servers bind to `127.0.0.1` only.
- Remote management is disabled by default in the shipped `config.yaml`.
- Secrets are encrypted at rest on Windows using DPAPI (per-user). Treat local files as sensitive anyway.
