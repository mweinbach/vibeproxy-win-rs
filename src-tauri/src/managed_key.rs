use crate::auth_manager;
use crate::secure_store;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

const MANAGED_KEY_FILE: &str = "codeforwarder-managed-remote-key.json";

#[derive(Debug, Serialize, Deserialize)]
struct ManagedKeyFile {
    key: String,
    key_encrypted: bool,
    created_at: String,
}

fn managed_key_path() -> PathBuf {
    auth_manager::get_auth_dir().join(MANAGED_KEY_FILE)
}

pub fn get_or_create_management_key() -> Result<String, String> {
    let path = managed_key_path();

    if let Ok(contents) = fs::read_to_string(&path) {
        if let Ok(mut file) = serde_json::from_str::<ManagedKeyFile>(&contents) {
            if file.key_encrypted {
                if let Ok(key) = secure_store::decrypt_secret(&file.key) {
                    if !key.is_empty() {
                        return Ok(key);
                    }
                }
            } else if !file.key.is_empty() {
                let plaintext = file.key.clone();
                // Backward compatibility for plaintext migration.
                if let Ok(encrypted) = secure_store::encrypt_secret(&file.key) {
                    file.key = encrypted;
                    file.key_encrypted = true;
                    let rendered = serde_json::to_string_pretty(&file)
                        .map_err(|e| format!("Failed to serialize managed key file: {}", e))?;
                    fs::write(&path, rendered)
                        .map_err(|e| format!("Failed to migrate managed key file: {}", e))?;
                }
                return Ok(plaintext);
            }
        }
    }

    let key = Uuid::new_v4().to_string();
    let encrypted =
        secure_store::encrypt_secret(&key).map_err(|e| format!("Failed to encrypt key: {}", e))?;
    let payload = ManagedKeyFile {
        key: encrypted,
        key_encrypted: true,
        created_at: Utc::now().to_rfc3339(),
    };
    let rendered = serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("Failed to serialize managed key file: {}", e))?;
    fs::write(&path, rendered).map_err(|e| format!("Failed to write managed key file: {}", e))?;

    Ok(key)
}
