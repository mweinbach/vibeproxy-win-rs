# Troubleshooting

## App says runtime is missing

1. Run:

```bash
bun run sync:cli-proxy-binary
```

2. Confirm bundled binary exists:

- macOS/Linux: `src-tauri/resources/cli-proxy-api-plus`
- Windows: `src-tauri/resources/cli-proxy-api-plus.exe`

3. If download/build is offline, set `SKIP_CLI_PROXY_SYNC=1` before `tauri build`.

## Server won’t start

Common causes:

- Port `8317` or `8318` already in use by another process.
- Existing stale runtime process from a previous run.
- Missing/corrupted runtime binary.

What to check:

- Restart the app and try Start again.
- Ensure no other app is binding ports 8317/8318.
- Re-sync the binary via `bun run sync:cli-proxy-binary`.

## Accounts not appearing in UI

- Verify auth files are in `~/.cli-proxy-api/`.
- Only valid JSON files with recognized `type` values are shown.
- The app watches that directory; if needed, reopen the app to force a rescan.

## Vercel gateway not being used

In Services → Claude:

- Confirm **Use Vercel AI Gateway** is enabled.
- Confirm API key is present and saved.
- Routing only applies to Claude-model requests.

## Z.AI models not available

- Add a Z.AI key in Services.
- Ensure Z.AI provider is enabled.
- VibeProxy generates `~/.cli-proxy-api/merged-config.yaml`; check that it includes `openai-compatibility` with `name: zai`.

## Build failures

- Ensure Bun and Rust are installed.
- Ensure Node.js version supports global `fetch` (Node 18+).
- Retry from a clean state:

```bash
bun install
bun run build
```

For desktop builds:

```bash
bun run tauri build
```
