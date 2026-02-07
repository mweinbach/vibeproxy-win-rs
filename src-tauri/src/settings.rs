use crate::types::AppSettings;
use tauri_plugin_store::StoreExt;

pub fn load_settings(app: &tauri::AppHandle) -> AppSettings {
    let store = match app.store("settings.json") {
        Ok(store) => store,
        Err(e) => {
            log::error!("[Settings] Failed to access store: {}", e);
            return AppSettings::default();
        }
    };

    let Some(value) = store.get("settings") else {
        return AppSettings::default();
    };

    let mut settings = serde_json::from_value::<AppSettings>(value.clone()).unwrap_or_default();
    let mut needs_migration = false;
    if let Some(obj) = value.as_object() {
        let is_encrypted = obj
            .get("vercel_api_key_encrypted")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if let Some(stored_key) = obj.get("vercel_api_key").and_then(|v| v.as_str()) {
            if is_encrypted {
                match crate::secure_store::decrypt_secret(stored_key) {
                    Ok(decrypted) => settings.vercel_api_key = decrypted,
                    Err(e) => {
                        log::error!("[Settings] Failed to decrypt Vercel API key: {}", e);
                        settings.vercel_api_key.clear();
                    }
                }
            } else {
                // Backward compatibility for legacy plaintext settings.
                settings.vercel_api_key = stored_key.to_string();
                needs_migration = !stored_key.is_empty();
            }
        }
    }

    if needs_migration {
        if let Err(e) = save_settings(app, &settings) {
            log::warn!("[Settings] Failed to migrate plaintext Vercel key: {}", e);
        }
    }

    settings
}

pub fn save_settings(app: &tauri::AppHandle, settings: &AppSettings) -> Result<(), String> {
    let store = app
        .store("settings.json")
        .map_err(|e| format!("Failed to access settings store: {}", e))?;

    let encrypted_key = crate::secure_store::encrypt_secret(&settings.vercel_api_key)?;
    let value = serde_json::json!({
        "enabled_providers": settings.enabled_providers,
        "vercel_gateway_enabled": settings.vercel_gateway_enabled,
        "vercel_api_key": encrypted_key,
        "vercel_api_key_encrypted": !settings.vercel_api_key.is_empty(),
        "launch_at_login": settings.launch_at_login
    });

    store.set("settings", value);
    Ok(())
}
