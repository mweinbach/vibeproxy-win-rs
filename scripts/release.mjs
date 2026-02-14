#!/usr/bin/env node

import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const ROOT = resolve(import.meta.dirname, "..");

const FILES = {
  package: resolve(ROOT, "package.json"),
  tauriConf: resolve(ROOT, "src-tauri/tauri.conf.json"),
  cargo: resolve(ROOT, "src-tauri/Cargo.toml"),
};

function run(cmd, { silent = false } = {}) {
  try {
    return execSync(cmd, { encoding: "utf8", stdio: silent ? "pipe" : "inherit" }).trim();
  } catch (err) {
    if (!silent) console.error(`Command failed: ${cmd}`);
    throw err;
  }
}

function runSilent(cmd) {
  return run(cmd, { silent: true });
}

function getCurrentVersion() {
  const pkg = JSON.parse(readFileSync(FILES.package, "utf8"));
  return pkg.version;
}

function bumpVersion(current, type = "patch") {
  const parts = current.split(".").map(Number);
  if (type === "major") {
    parts[0]++;
    parts[1] = 0;
    parts[2] = 0;
  } else if (type === "minor") {
    parts[1]++;
    parts[2] = 0;
  } else {
    parts[2]++;
  }
  return parts.join(".");
}

function updateVersionFiles(newVersion) {
  const pkg = JSON.parse(readFileSync(FILES.package, "utf8"));
  pkg.version = newVersion;
  writeFileSync(FILES.package, JSON.stringify(pkg, null, 2) + "\n");

  const tauriConf = JSON.parse(readFileSync(FILES.tauriConf, "utf8"));
  tauriConf.version = newVersion;
  writeFileSync(FILES.tauriConf, JSON.stringify(tauriConf, null, 2) + "\n");

  let cargo = readFileSync(FILES.cargo, "utf8");
  cargo = cargo.replace(/^version\s*=\s*"[^"]+"/m, `version = "${newVersion}"`);
  writeFileSync(FILES.cargo, cargo);
}

function hasUncommittedChanges() {
  const status = runSilent("git status --porcelain");
  return status.length > 0;
}

function main() {
  const args = process.argv.slice(2);
  const bumpType = args[0] || "patch";
  const notes = args.slice(1).join(" ") || "";

  if (!["major", "minor", "patch"].includes(bumpType)) {
    console.error(`Usage: node release.mjs [major|minor|patch] [release notes...]`);
    console.error(`  Default: patch`);
    process.exit(1);
  }

  const current = getCurrentVersion();
  const next = bumpVersion(current, bumpType);
  const tag = `v${next}`;

  console.log(`\nReleasing ${tag} (from ${current})\n`);

  if (hasUncommittedChanges()) {
    console.log("Committing uncommitted changes...");
    run("git add -A");
    run(`git commit -m "chore: prepare for release ${tag}"`);
  }

  console.log("Updating version files...");
  updateVersionFiles(next);

  console.log("Committing version bump...");
  run("git add -A");
  run(`git commit -m "release: ${tag}"`);

  console.log("Creating tag...");
  run(`git tag ${tag}`);

  console.log("Pushing to remote...");
  run("git push origin main --tags");

  console.log("Creating GitHub release...");
  const releaseNotes = notes || `Release ${tag}`;
  run(`gh release create ${tag} --title "VibeProxy ${tag}" --notes "${releaseNotes}"`);

  console.log("Triggering build workflows...");
  run(`gh workflow run "Build macOS (Tauri)" -f ref=${tag} -f release_tag=${tag}`);
  run(`gh workflow run "Build Windows (Tauri)" -f ref=${tag} -f release_tag=${tag}`);
  run(`gh workflow run "Build Linux (Tauri)" -f ref=${tag} -f release_tag=${tag}`);

  console.log(`\nDone! Release ${tag} created and builds triggered.`);
  console.log(`https://github.com/mweinbach/vibeproxy-win-rs/releases/tag/${tag}\n`);
}

main();