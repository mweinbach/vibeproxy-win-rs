import { createWriteStream } from "node:fs";
import {
  access,
  chmod,
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
import { fileURLToPath } from "node:url";

const REPO_OWNER = "router-for-me";
const REPO_NAME = "CLIProxyAPIPlus";
const RELEASE_URL = `https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases/latest`;

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");
const OUTPUT_DIR = path.join(REPO_ROOT, "src-tauri", "resources");
const OUTPUT_VERSION = path.join(OUTPUT_DIR, "cli-proxy-api-plus.version");

const TMP_EXTRACT_DIR = path.join(OUTPUT_DIR, "cli-proxy-api-plus.extract.tmp");

const USER_AGENT = "vibeproxy-win-build-sync";

function resolveTarget() {
  const arch = (() => {
    if (process.arch === "x64") return "amd64";
    if (process.arch === "arm64") return "arm64";
    return null;
  })();

  if (!arch) {
    throw new Error(`[sync-cli-proxy-binary] Unsupported arch: ${process.arch}`);
  }

  if (process.platform === "win32") {
    return {
      assetSuffix: `_windows_${arch}.zip`,
      binaryName: "cli-proxy-api-plus.exe",
      archiveExt: ".zip",
      extractedName: "cli-proxy-api-plus.exe",
    };
  }

  if (process.platform === "darwin") {
    return {
      assetSuffix: `_darwin_${arch}.tar.gz`,
      binaryName: "cli-proxy-api-plus",
      archiveExt: ".tar.gz",
      extractedName: "cli-proxy-api-plus",
    };
  }

  if (process.platform === "linux") {
    return {
      assetSuffix: `_linux_${arch}.tar.gz`,
      binaryName: "cli-proxy-api-plus",
      archiveExt: ".tar.gz",
      extractedName: "cli-proxy-api-plus",
    };
  }

  return null;
}

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

function resolveAsset(release, assetSuffix) {
  const assets = Array.isArray(release.assets) ? release.assets : [];
  const asset = assets.find(
    (item) =>
      item &&
      typeof item.name === "string" &&
      item.name.endsWith(assetSuffix) &&
      typeof item.browser_download_url === "string",
  );

  if (!asset) {
    throw new Error(
      `Could not find release asset ending with '${assetSuffix}' on ${release.tag_name}`,
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

function expandZipWithPowershell(zipPath, destinationPath) {
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

function extractTarGz(tarPath, destinationPath, extractedName) {
  const result = spawnSync(
    "tar",
    ["-xzf", tarPath, "-C", destinationPath, extractedName],
    { encoding: "utf8", stdio: "pipe" },
  );

  if (result.status !== 0) {
    throw new Error(
      `tar extraction failed with code ${result.status}: ${
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

  const target = resolveTarget();
  if (!target) {
    console.log(
      `[sync-cli-proxy-binary] Skipped: unsupported platform (${process.platform})`,
    );
    return;
  }

  await mkdir(OUTPUT_DIR, { recursive: true });

  const OUTPUT_BINARY = path.join(OUTPUT_DIR, target.binaryName);
  const TMP_ARCHIVE = path.join(
    OUTPUT_DIR,
    `cli-proxy-api-plus.download${target.archiveExt}`,
  );

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

  const asset = resolveAsset(release, target.assetSuffix);
  console.log(
    `[sync-cli-proxy-binary] Downloading ${asset.name} (${version})...`,
  );

  await downloadFile(asset.url, TMP_ARCHIVE);

  await rm(TMP_EXTRACT_DIR, { recursive: true, force: true });
  await mkdir(TMP_EXTRACT_DIR, { recursive: true });
  if (target.archiveExt === ".zip") {
    expandZipWithPowershell(TMP_ARCHIVE, TMP_EXTRACT_DIR);
  } else {
    extractTarGz(TMP_ARCHIVE, TMP_EXTRACT_DIR, target.extractedName);
  }

  const extractedBinary = path.join(TMP_EXTRACT_DIR, target.extractedName);
  if (!(await fileExists(extractedBinary))) {
    throw new Error(
      `Expected ${target.extractedName} inside archive but it was not found`,
    );
  }

  await rm(OUTPUT_BINARY, { force: true });
  await rename(extractedBinary, OUTPUT_BINARY);
  if (process.platform !== "win32") {
    await chmod(OUTPUT_BINARY, 0o755);
  }
  await writeFile(OUTPUT_VERSION, `${version}\n`, "utf8");

  await rm(TMP_ARCHIVE, { force: true });
  await rm(TMP_EXTRACT_DIR, { recursive: true, force: true });

  console.log(
    `[sync-cli-proxy-binary] Synced ${version} -> ${OUTPUT_BINARY}`,
  );
}

main().catch(async (error) => {
  // Best-effort cleanup. TMP_ARCHIVE is platform-dependent so we clean by prefix.
  await rm(path.join(OUTPUT_DIR, "cli-proxy-api-plus.download.zip"), {
    force: true,
  }).catch(() => {});
  await rm(path.join(OUTPUT_DIR, "cli-proxy-api-plus.download.tar.gz"), {
    force: true,
  }).catch(() => {});
  await rm(TMP_EXTRACT_DIR, { recursive: true, force: true }).catch(() => {});
  console.error("[sync-cli-proxy-binary] Failed:", error);
  process.exit(1);
});
