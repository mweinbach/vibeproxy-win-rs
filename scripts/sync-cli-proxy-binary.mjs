import { createWriteStream } from "node:fs";
import {
  access,
  constants,
  mkdir,
  readFile,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";

const REPO_OWNER = "router-for-me";
const REPO_NAME = "CLIProxyAPIPlus";
const RELEASE_URL = `https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases/latest`;

const ASSET_SUFFIX = "_windows_amd64.zip";
const OUTPUT_DIR = path.resolve(process.cwd(), "src-tauri", "resources");
const OUTPUT_BINARY = path.join(OUTPUT_DIR, "cli-proxy-api-plus.exe");
const OUTPUT_VERSION = path.join(OUTPUT_DIR, "cli-proxy-api-plus.version");

const TMP_ZIP = path.join(OUTPUT_DIR, "cli-proxy-api-plus.download.zip");
const TMP_EXTRACT_DIR = path.join(OUTPUT_DIR, "cli-proxy-api-plus.extract.tmp");

const USER_AGENT = "vibeproxy-win-build-sync";

async function fileExists(filePath) {
  try {
    await access(filePath, constants.F_OK);
    return true;
  } catch {
    return false;
  }
}

async function readExistingVersion() {
  try {
    return (await readFile(OUTPUT_VERSION, "utf8")).trim();
  } catch {
    return "";
  }
}

async function fetchJson(url) {
  const response = await fetch(url, {
    headers: {
      "User-Agent": USER_AGENT,
      Accept: "application/vnd.github+json",
    },
  });

  if (!response.ok) {
    throw new Error(
      `Request failed (${response.status} ${response.statusText}) for ${url}`,
    );
  }

  return response.json();
}

function resolveAsset(release) {
  const assets = Array.isArray(release.assets) ? release.assets : [];
  const asset = assets.find(
    (item) =>
      item &&
      typeof item.name === "string" &&
      item.name.endsWith(ASSET_SUFFIX) &&
      typeof item.browser_download_url === "string",
  );

  if (!asset) {
    throw new Error(
      `Could not find release asset ending with '${ASSET_SUFFIX}' on ${release.tag_name}`,
    );
  }

  return {
    name: asset.name,
    url: asset.browser_download_url,
  };
}

async function downloadFile(url, outputFile) {
  const response = await fetch(url, {
    headers: {
      "User-Agent": USER_AGENT,
      Accept: "application/octet-stream",
    },
    redirect: "follow",
  });

  if (!response.ok) {
    throw new Error(
      `Download failed (${response.status} ${response.statusText}) from ${url}`,
    );
  }

  if (!response.body) {
    throw new Error("Download response body is empty");
  }

  await pipeline(Readable.fromWeb(response.body), createWriteStream(outputFile));
}

function expandArchive(zipPath, destinationPath) {
  const escapedZip = zipPath.replace(/'/g, "''");
  const escapedDestination = destinationPath.replace(/'/g, "''");
  const script =
    `Expand-Archive -Path '${escapedZip}' ` +
    `-DestinationPath '${escapedDestination}' -Force`;

  const result = spawnSync("powershell", ["-NoProfile", "-Command", script], {
    encoding: "utf8",
    stdio: "pipe",
  });

  if (result.status !== 0) {
    throw new Error(
      `Expand-Archive failed with code ${result.status}: ${
        result.stderr || result.stdout || "unknown error"
      }`,
    );
  }
}

async function main() {
  if (process.env.SKIP_CLI_PROXY_SYNC === "1") {
    console.log("[sync-cli-proxy-binary] Skipped via SKIP_CLI_PROXY_SYNC=1");
    return;
  }

  if (process.platform !== "win32") {
    console.log(
      "[sync-cli-proxy-binary] Skipped: Windows binary sync only runs on win32",
    );
    return;
  }

  await mkdir(OUTPUT_DIR, { recursive: true });

  const release = await fetchJson(RELEASE_URL);
  const version =
    release && typeof release.tag_name === "string" ? release.tag_name : "";
  if (!version) {
    throw new Error("Could not resolve latest release tag_name from GitHub API");
  }

  const existingVersion = await readExistingVersion();
  if (existingVersion === version && (await fileExists(OUTPUT_BINARY))) {
    console.log(
      `[sync-cli-proxy-binary] Already up-to-date (${version}): ${OUTPUT_BINARY}`,
    );
    return;
  }

  const asset = resolveAsset(release);
  console.log(
    `[sync-cli-proxy-binary] Downloading ${asset.name} (${version})...`,
  );

  await downloadFile(asset.url, TMP_ZIP);

  await rm(TMP_EXTRACT_DIR, { recursive: true, force: true });
  await mkdir(TMP_EXTRACT_DIR, { recursive: true });
  expandArchive(TMP_ZIP, TMP_EXTRACT_DIR);

  const extractedBinary = path.join(TMP_EXTRACT_DIR, "cli-proxy-api-plus.exe");
  if (!(await fileExists(extractedBinary))) {
    throw new Error(
      "Expected cli-proxy-api-plus.exe inside archive but it was not found",
    );
  }

  await rm(OUTPUT_BINARY, { force: true });
  await rename(extractedBinary, OUTPUT_BINARY);
  await writeFile(OUTPUT_VERSION, `${version}\n`, "utf8");

  await rm(TMP_ZIP, { force: true });
  await rm(TMP_EXTRACT_DIR, { recursive: true, force: true });

  console.log(
    `[sync-cli-proxy-binary] Synced ${version} -> ${OUTPUT_BINARY}`,
  );
}

main().catch(async (error) => {
  await rm(TMP_ZIP, { force: true }).catch(() => {});
  await rm(TMP_EXTRACT_DIR, { recursive: true, force: true }).catch(() => {});
  console.error("[sync-cli-proxy-binary] Failed:", error);
  process.exit(1);
});
