use crate::types::BinaryDownloadProgress;
use std::{path::PathBuf, sync::OnceLock, time::Duration};
use tauri::Emitter;
use tauri::Manager;

const BUNDLED_BINARY_NAME: &str = "cli-proxy-api-plus.exe";
const BUNDLED_BINARY_NESTED_PATH: &str = "resources/cli-proxy-api-plus.exe";
const RELEASES_API_URL: &str =
    "https://api.github.com/repos/router-for-me/CLIProxyAPIPlus/releases/latest";
const RELEASE_LOOKUP_TIMEOUT_SECS: u64 = 15;
const DOWNLOAD_CONNECT_TIMEOUT_SECS: u64 = 10;
const DOWNLOAD_READ_TIMEOUT_SECS: u64 = 30;

pub struct ReleaseInfo {
    pub asset_name: String,
    pub download_url: String,
    pub sha256: String,
}

fn looks_like_sha256(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

fn release_lookup_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(DOWNLOAD_CONNECT_TIMEOUT_SECS))
            .timeout(Duration::from_secs(RELEASE_LOOKUP_TIMEOUT_SECS))
            .build()
            .expect("Failed to build release lookup client")
    })
}

fn binary_download_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(DOWNLOAD_CONNECT_TIMEOUT_SECS))
            .read_timeout(Duration::from_secs(DOWNLOAD_READ_TIMEOUT_SECS))
            .pool_idle_timeout(Duration::from_secs(60))
            .tcp_nodelay(true)
            .build()
            .expect("Failed to build binary download client")
    })
}

pub fn get_binary_path() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    base.join("vibeproxy").join(BUNDLED_BINARY_NAME)
}

pub fn get_bundled_binary_path(app_handle: &tauri::AppHandle) -> Option<PathBuf> {
    let resource_dir = app_handle.path().resource_dir().ok()?;

    let nested = resource_dir.join(BUNDLED_BINARY_NESTED_PATH);
    if nested.exists() {
        return Some(nested);
    }

    let flat = resource_dir.join(BUNDLED_BINARY_NAME);
    if flat.exists() {
        return Some(flat);
    }

    None
}

pub fn is_binary_available_for_app(app_handle: &tauri::AppHandle) -> bool {
    get_binary_path().exists() || get_bundled_binary_path(app_handle).is_some()
}

pub fn ensure_binary_installed(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let local_path = get_binary_path();
    if local_path.exists() {
        return Ok(local_path);
    }

    let bundled_path = get_bundled_binary_path(app_handle)
        .ok_or_else(|| "Binary not available. Please download it first.".to_string())?;

    let parent = local_path
        .parent()
        .ok_or_else(|| "Could not determine binary parent directory".to_string())?;

    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create binary directory: {}", e))?;

    match std::fs::copy(&bundled_path, &local_path) {
        Ok(_) => Ok(local_path),
        Err(e) => {
            log::warn!(
                "[BinaryManager] Could not copy bundled binary to local dir: {}. Using bundled path directly.",
                e
            );
            Ok(bundled_path)
        }
    }
}

pub async fn get_latest_release_info() -> Result<ReleaseInfo, String> {
    let client = release_lookup_client();
    let resp = client
        .get(RELEASES_API_URL)
        .header("User-Agent", "vibeproxy-win")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch latest release: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("GitHub API returned status {}", resp.status()));
    }

    let json = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse release JSON: {}", e))?;

    let version = json
        .get("tag_name")
        .and_then(|v: &serde_json::Value| v.as_str())
        .map(String::from)
        .ok_or_else(|| "tag_name not found in release response".to_string())?;

    let asset_version = version.strip_prefix('v').unwrap_or(&version);
    let asset_name = format!("CLIProxyAPIPlus_{}_windows_amd64.zip", asset_version);

    let assets = json
        .get("assets")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "assets not found in release response".to_string())?;

    let zip_asset = assets
        .iter()
        .find(|asset| asset.get("name").and_then(|v| v.as_str()) == Some(asset_name.as_str()))
        .ok_or_else(|| format!("Release asset not found: {}", asset_name))?;

    let download_url = zip_asset
        .get("browser_download_url")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| format!("Download URL missing for release asset: {}", asset_name))?;

    let sha256 = if let Some(digest) = zip_asset.get("digest").and_then(|v| v.as_str()) {
        // GitHub often exposes digest as "sha256:<hex>"
        digest
            .split_once(':')
            .map(|(_, hash)| hash.to_ascii_lowercase())
            .unwrap_or_else(|| digest.to_ascii_lowercase())
    } else {
        let checksum_manifest_url = assets
            .iter()
            .find(|asset| {
                asset
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|name| {
                        let name = name.to_ascii_lowercase();
                        name.contains("sha256")
                            || name.contains("checksums")
                            || name.contains("checksum")
                    })
                    .unwrap_or(false)
            })
            .and_then(|asset| asset.get("browser_download_url").and_then(|v| v.as_str()))
            .ok_or_else(|| "Checksum manifest not found in latest release".to_string())?;

        let checksum_manifest = client
            .get(checksum_manifest_url)
            .header("User-Agent", "vibeproxy-win")
            .send()
            .await
            .map_err(|e| format!("Failed to download checksum manifest: {}", e))?;

        if !checksum_manifest.status().is_success() {
            return Err(format!(
                "Checksum manifest download failed with status {}",
                checksum_manifest.status()
            ));
        }

        let checksum_manifest = checksum_manifest
            .text()
            .await
            .map_err(|e| format!("Failed to read checksum manifest: {}", e))?;

        extract_sha256_for_asset(&checksum_manifest, &asset_name).ok_or_else(|| {
            format!(
                "SHA-256 for {} not found in release checksum manifest",
                asset_name
            )
        })?
    };

    if !looks_like_sha256(&sha256) {
        return Err(format!(
            "Invalid SHA-256 value for {}: {}",
            asset_name, sha256
        ));
    }

    Ok(ReleaseInfo {
        asset_name,
        download_url,
        sha256,
    })
}

pub async fn download_binary(
    app_handle: tauri::AppHandle,
    release: &ReleaseInfo,
) -> Result<String, String> {
    let client = binary_download_client();
    let resp = client
        .get(&release.download_url)
        .header("User-Agent", "vibeproxy-win")
        .send()
        .await
        .map_err(|e| format!("Failed to start download: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Download failed with status {}", resp.status()));
    }

    let total_bytes = resp.content_length().unwrap_or(0);
    let mut bytes_downloaded: u64 = 0;

    let binary_path = get_binary_path();
    let parent = binary_path
        .parent()
        .ok_or_else(|| "Could not determine binary parent directory".to_string())?;

    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|e| format!("Failed to create directory: {}", e))?;

    let temp_zip_path = parent.join("cli-proxy-api-plus.zip.tmp");
    let temp_exe_path = parent.join("cli-proxy-api-plus.exe.tmp");

    let mut file = tokio::fs::File::create(&temp_zip_path)
        .await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    use futures_util::StreamExt;
    use sha2::Digest;
    use sha2::Sha256;
    use tokio::io::AsyncWriteExt;

    let mut hasher = Sha256::new();
    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Error reading download stream: {}", e))?;
        hasher.update(&chunk);

        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Failed to write chunk: {}", e))?;

        bytes_downloaded += chunk.len() as u64;

        let progress = if total_bytes > 0 {
            (bytes_downloaded as f64 / total_bytes as f64) * 100.0
        } else {
            0.0
        };

        app_handle
            .emit(
                "binary_download_progress",
                BinaryDownloadProgress {
                    progress,
                    bytes_downloaded,
                    total_bytes,
                },
            )
            .ok();
    }

    file.flush()
        .await
        .map_err(|e| format!("Failed to flush file: {}", e))?;

    drop(file);

    let actual_sha256 = format!("{:x}", hasher.finalize());
    if actual_sha256 != release.sha256.to_ascii_lowercase() {
        let _ = tokio::fs::remove_file(&temp_zip_path).await;
        let _ = tokio::fs::remove_file(&temp_exe_path).await;
        return Err(format!(
            "Binary checksum mismatch for {}. Expected {}, got {}",
            release.asset_name, release.sha256, actual_sha256
        ));
    }

    let zip_for_extract = temp_zip_path.clone();
    let exe_for_extract = temp_exe_path.clone();
    tokio::task::spawn_blocking(move || {
        extract_binary_from_zip(&zip_for_extract, &exe_for_extract)
    })
    .await
    .map_err(|e| format!("Failed to join archive extraction task: {}", e))??;

    tokio::fs::rename(&temp_exe_path, &binary_path)
        .await
        .map_err(|e| format!("Failed to move extracted binary into place: {}", e))?;

    let _ = tokio::fs::remove_file(&temp_zip_path).await;

    Ok(binary_path.to_string_lossy().to_string())
}

fn extract_sha256_for_asset(manifest: &str, asset_name: &str) -> Option<String> {
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.contains(asset_name) {
            continue;
        }

        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.len() >= 2 {
            // Pattern: "<sha256>  <filename>"
            if looks_like_sha256(tokens[0]) && trimmed.contains(asset_name) {
                return Some(tokens[0].to_ascii_lowercase());
            }

            // Pattern: "<filename>  <sha256>"
            if looks_like_sha256(tokens[tokens.len() - 1]) {
                return Some(tokens[tokens.len() - 1].to_ascii_lowercase());
            }
        }

        // Pattern: "<filename>: <sha256>"
        if let Some((left, right)) = trimmed.split_once(':') {
            if left.contains(asset_name) && looks_like_sha256(right.trim()) {
                return Some(right.trim().to_ascii_lowercase());
            }
            if right.contains(asset_name) && looks_like_sha256(left.trim()) {
                return Some(left.trim().to_ascii_lowercase());
            }
        }
    }

    None
}

fn extract_binary_from_zip(
    zip_path: &std::path::Path,
    output_path: &std::path::Path,
) -> Result<(), String> {
    use std::io;

    let input = std::fs::File::open(zip_path)
        .map_err(|e| format!("Failed to open downloaded archive: {}", e))?;

    let mut archive = zip::ZipArchive::new(input)
        .map_err(|e| format!("Failed to parse downloaded archive: {}", e))?;

    let mut binary_file = archive
        .by_name("cli-proxy-api-plus.exe")
        .map_err(|e| format!("Binary not found in archive: {}", e))?;

    let mut output = std::fs::File::create(output_path)
        .map_err(|e| format!("Failed to create extracted binary file: {}", e))?;

    io::copy(&mut binary_file, &mut output)
        .map_err(|e| format!("Failed to write extracted binary: {}", e))?;

    Ok(())
}
