use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::auth_manager;

pub fn get_base_config_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    let resource_dir = app_handle
        .path()
        .resource_dir()
        .map_err(|e| format!("Failed to resolve resource dir: {}", e))?;
    Ok(resource_dir.join("resources").join("config.yaml"))
}

pub fn get_merged_config_path(
    app_handle: &tauri::AppHandle,
    enabled_providers: &HashMap<String, bool>,
) -> Result<PathBuf, String> {
    let auth_dir = auth_manager::get_auth_dir();
    let base_config_path = get_base_config_path(app_handle)?;

    // Scan for zai-*.json files and extract api_key values
    let mut zai_keys: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(&auth_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if file_name.starts_with("zai-") && file_name.ends_with(".json") {
                if let Ok(contents) = fs::read_to_string(&path) {
                    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&contents) {
                        let encrypted = json
                            .get("api_key_encrypted")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        if let Some(stored_key) = json
                            .get("api_key")
                            .and_then(|v| v.as_str())
                            .map(str::to_string)
                        {
                            let resolved_key = if encrypted {
                                match crate::secure_store::decrypt_secret(&stored_key) {
                                    Ok(k) => k,
                                    Err(e) => {
                                        log::warn!(
                                            "[ConfigManager] Failed to decrypt Z.AI key in {:?}: {}",
                                            path,
                                            e
                                        );
                                        String::new()
                                    }
                                }
                            } else {
                                // Backward compatibility for legacy plaintext keys.
                                if !stored_key.is_empty() {
                                    if let Ok(encrypted_key) =
                                        crate::secure_store::encrypt_secret(&stored_key)
                                    {
                                        if let Some(obj) = json.as_object_mut() {
                                            obj.insert(
                                                "api_key".to_string(),
                                                serde_json::Value::String(encrypted_key),
                                            );
                                            obj.insert(
                                                "api_key_encrypted".to_string(),
                                                serde_json::Value::Bool(true),
                                            );
                                            if let Ok(serialized) = serde_json::to_vec_pretty(&json)
                                            {
                                                let _ = fs::write(&path, serialized);
                                            }
                                        }
                                    }
                                }
                                stored_key
                            };

                            if !resolved_key.is_empty() {
                                zai_keys.push(resolved_key);
                            }
                        }
                    }
                }
            }
        }
    }

    // Build disabled providers list
    let disabled_providers: Vec<String> = enabled_providers
        .iter()
        .filter(|(_, enabled)| !**enabled)
        .map(|(key, _)| key.clone())
        .collect();

    // If no Z.AI keys and no disabled providers, return the base config path
    if zai_keys.is_empty() && disabled_providers.is_empty() {
        return Ok(base_config_path);
    }

    // Read and parse the base config.
    let base_config = fs::read_to_string(&base_config_path)
        .map_err(|e| format!("Failed to read base config: {}", e))?;
    let mut root: serde_yaml::Value = serde_yaml::from_str(&base_config)
        .map_err(|e| format!("Failed to parse base config YAML: {}", e))?;
    let root_map = root
        .as_mapping_mut()
        .ok_or_else(|| "Base config root must be a YAML mapping".to_string())?;

    // Apply oauth-excluded-models section for disabled providers.
    if !disabled_providers.is_empty() {
        let provider_key_to_oauth_key: HashMap<&str, &str> = HashMap::from([
            ("claude", "claude"),
            ("codex", "codex"),
            ("gemini", "gemini-cli"),
            ("github-copilot", "github-copilot"),
            ("antigravity", "antigravity"),
            ("qwen", "qwen"),
        ]);

        let mut oauth_keys: Vec<String> = Vec::new();
        for provider_key in &disabled_providers {
            if let Some(oauth_key) = provider_key_to_oauth_key.get(provider_key.as_str()) {
                oauth_keys.push(oauth_key.to_string());
            }
        }

        if !oauth_keys.is_empty() {
            oauth_keys.sort();

            let section_key = serde_yaml::Value::String("oauth-excluded-models".to_string());
            if !matches!(
                root_map.get(&section_key),
                Some(serde_yaml::Value::Mapping(_))
            ) {
                root_map.insert(
                    section_key.clone(),
                    serde_yaml::Value::Mapping(Default::default()),
                );
            }

            let section = root_map
                .get_mut(&section_key)
                .and_then(|v| v.as_mapping_mut())
                .ok_or_else(|| "oauth-excluded-models must be a YAML mapping".to_string())?;

            for key in &oauth_keys {
                section.insert(
                    serde_yaml::Value::String(key.clone()),
                    serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("*".to_string())]),
                );
            }
        }
    }

    // Apply openai-compatibility section for Z.AI keys (if enabled).
    let zai_enabled = enabled_providers.get("zai").copied().unwrap_or(true);
    if !zai_keys.is_empty() && zai_enabled {
        let section_key = serde_yaml::Value::String("openai-compatibility".to_string());
        if !matches!(
            root_map.get(&section_key),
            Some(serde_yaml::Value::Sequence(_))
        ) {
            root_map.insert(section_key.clone(), serde_yaml::Value::Sequence(Vec::new()));
        }

        let section = root_map
            .get_mut(&section_key)
            .and_then(|v| v.as_sequence_mut())
            .ok_or_else(|| "openai-compatibility must be a YAML sequence".to_string())?;

        // Remove any previously injected zai entries so config stays deterministic.
        section.retain(|entry| {
            entry
                .as_mapping()
                .and_then(|m| m.get(&serde_yaml::Value::String("name".to_string())))
                .and_then(|v| v.as_str())
                != Some("zai")
        });

        let mut zai_entry = serde_yaml::Mapping::new();
        zai_entry.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String("zai".to_string()),
        );
        zai_entry.insert(
            serde_yaml::Value::String("base-url".to_string()),
            serde_yaml::Value::String("https://api.z.ai/api/coding/paas/v4".to_string()),
        );

        let mut api_entries = Vec::new();
        for key in &zai_keys {
            let mut key_entry = serde_yaml::Mapping::new();
            key_entry.insert(
                serde_yaml::Value::String("api-key".to_string()),
                serde_yaml::Value::String(key.clone()),
            );
            api_entries.push(serde_yaml::Value::Mapping(key_entry));
        }
        zai_entry.insert(
            serde_yaml::Value::String("api-key-entries".to_string()),
            serde_yaml::Value::Sequence(api_entries),
        );

        let models = ["glm-4.7", "glm-4-plus", "glm-4-air", "glm-4-flash"]
            .iter()
            .map(|model| {
                let mut m = serde_yaml::Mapping::new();
                m.insert(
                    serde_yaml::Value::String("name".to_string()),
                    serde_yaml::Value::String((*model).to_string()),
                );
                m.insert(
                    serde_yaml::Value::String("alias".to_string()),
                    serde_yaml::Value::String((*model).to_string()),
                );
                serde_yaml::Value::Mapping(m)
            })
            .collect::<Vec<_>>();
        zai_entry.insert(
            serde_yaml::Value::String("models".to_string()),
            serde_yaml::Value::Sequence(models),
        );

        section.push(serde_yaml::Value::Mapping(zai_entry));
    }

    // Write merged config.
    let merged_path = auth_dir.join("merged-config.yaml");
    let rendered = serde_yaml::to_string(&root)
        .map_err(|e| format!("Failed to serialize merged YAML: {}", e))?;
    fs::write(&merged_path, rendered)
        .map_err(|e| format!("Failed to write merged config: {}", e))?;

    Ok(merged_path)
}
