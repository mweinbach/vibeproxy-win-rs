use crate::types::{
    AgentInstallResult, FactoryCustomModelInput, FactoryCustomModelRow,
    FactoryCustomModelsRemoveResult, FactoryCustomModelsState,
};
use chrono::Utc;
use reqwest::Url;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn factory_settings_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    Ok(home.join(".factory").join("settings.json"))
}

fn normalize_key_part(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_ascii_lowercase()
}

fn model_dedup_key(model: &str, base_url: &str, provider: &str) -> (String, String, String) {
    (
        model.trim().to_ascii_lowercase(),
        normalize_key_part(base_url),
        provider.trim().to_ascii_lowercase(),
    )
}

fn agent_id_prefix(agent_key: &str) -> String {
    let trimmed = agent_key.trim().to_ascii_lowercase();
    format!("custom:{}:", trimmed)
}

fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_dash = false;
    for ch in input.chars() {
        let lower = ch.to_ascii_lowercase();
        let is_alnum = lower.is_ascii_alphanumeric();
        if is_alnum {
            out.push(lower);
            prev_dash = false;
            continue;
        }
        if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "model".to_string()
    } else {
        trimmed
    }
}

fn next_custom_model_index(existing: &[Value]) -> i64 {
    existing
        .iter()
        .filter_map(|v| v.get("index"))
        .filter_map(|v| v.as_i64())
        .max()
        .unwrap_or(-1)
        + 1
}

fn existing_custom_model_keys(existing: &[Value]) -> HashSet<(String, String, String)> {
    let mut keys = HashSet::new();
    for entry in existing {
        let Some(obj) = entry.as_object() else {
            continue;
        };
        let Some(model) = obj.get("model").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(base_url) = obj.get("baseUrl").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(provider) = obj.get("provider").and_then(|v| v.as_str()) else {
            continue;
        };
        keys.insert(model_dedup_key(model, base_url, provider));
    }
    keys
}

fn existing_custom_model_ids(existing: &[Value]) -> HashSet<String> {
    let mut ids = HashSet::new();
    for entry in existing {
        if let Some(id) = entry.get("id").and_then(|v| v.as_str()) {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                ids.insert(trimmed.to_string());
            }
        }
    }
    ids
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create parent directory {:?}: {}", parent, e))
}

fn read_json_file(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("Failed to read {:?}: {}", path, e))?;
    serde_json::from_str::<Value>(&text)
        .map_err(|e| format!("Failed to parse {:?} as JSON: {}", path, e))
}

fn write_json_atomic(path: &Path, value: &Value, create_backup: bool) -> Result<(), String> {
    ensure_parent_dir(path)?;

    if create_backup && path.exists() {
        let ts = Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let backup = path.with_file_name(format!(
            "{}.bak.{}",
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("settings.json"),
            ts
        ));
        fs::copy(path, &backup).map_err(|e| format!("Failed to create backup: {}", e))?;
    }

    let rendered = serde_json::to_vec_pretty(value)
        .map_err(|e| format!("Failed to serialize settings JSON: {}", e))?;

    let tmp_name = format!(
        "{}.tmp.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("settings.json"),
        Uuid::new_v4().to_string()
    );
    let tmp_path = path.with_file_name(tmp_name);
    fs::write(&tmp_path, rendered).map_err(|e| format!("Failed to write temp file: {}", e))?;

    match fs::rename(&tmp_path, path) {
        Ok(()) => Ok(()),
        Err(rename_err) => {
            // On Windows, rename fails if destination exists.
            if path.exists() {
                fs::remove_file(path).map_err(|e| {
                    format!("Failed to remove existing {:?} before replace: {}", path, e)
                })?;
                fs::rename(&tmp_path, path)
                    .map_err(|e| format!("Failed to replace {:?}: {}", path, e))?;
                Ok(())
            } else {
                let _ = fs::remove_file(&tmp_path);
                Err(format!("Failed to replace {:?}: {}", path, rename_err))
            }
        }
    }
}

fn is_proxy_base_url(base_url: &str) -> bool {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://localhost:8317")
        || lower.starts_with("https://localhost:8317")
        || lower.starts_with("http://127.0.0.1:8317")
        || lower.starts_with("https://127.0.0.1:8317")
        || lower.starts_with("http://0.0.0.0:8317")
        || lower.starts_with("https://0.0.0.0:8317")
    {
        return true;
    }

    if let Ok(url) = Url::parse(trimmed) {
        let port = url.port_or_known_default().unwrap_or(0);
        if port != 8317 {
            return false;
        }
        let host = url.host_str().unwrap_or("").to_ascii_lowercase();
        return host == "localhost" || host == "127.0.0.1" || host == "0.0.0.0" || host == "::1";
    }

    false
}

fn session_default_model_id(root: &Value) -> Option<String> {
    root.get("sessionDefaultSettings")
        .and_then(|v| v.get("model"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn parse_custom_model_row(
    entry: &Value,
    default_id: Option<&str>,
) -> Option<FactoryCustomModelRow> {
    let Some(obj) = entry.as_object() else {
        return None;
    };
    let id = obj.get("id")?.as_str()?.trim();
    if id.is_empty() {
        return None;
    }

    let model = obj
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let base_url = obj
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let display_name = obj
        .get("displayName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let provider = obj
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let no_image_support = obj
        .get("noImageSupport")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let index = obj.get("index").and_then(|v| v.as_i64());
    let is_proxy = is_proxy_base_url(&base_url);
    let is_session_default = default_id.map(|d| d == id).unwrap_or(false);

    Some(FactoryCustomModelRow {
        id: id.to_string(),
        index,
        model: if model.is_empty() {
            id.to_string()
        } else {
            model
        },
        base_url,
        display_name: if display_name.is_empty() {
            id.to_string()
        } else {
            display_name
        },
        no_image_support,
        provider,
        is_proxy,
        is_session_default,
    })
}

fn list_factory_custom_models_at_path(path: &Path) -> Result<FactoryCustomModelsState, String> {
    let factory_settings_path = path.to_string_lossy().to_string();
    if !path.exists() {
        return Ok(FactoryCustomModelsState {
            factory_settings_path,
            session_default_model: None,
            models: Vec::new(),
        });
    }

    let root = read_json_file(path)?;
    let default_model = session_default_model_id(&root);
    let default_ref = default_model.as_deref();
    let existing = root
        .get("customModels")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut models: Vec<FactoryCustomModelRow> = Vec::new();
    for entry in existing {
        if let Some(row) = parse_custom_model_row(&entry, default_ref) {
            models.push(row);
        }
    }

    models.sort_by(|a, b| {
        let ai = a.index.unwrap_or(i64::MAX);
        let bi = b.index.unwrap_or(i64::MAX);
        ai.cmp(&bi)
            .then_with(|| a.display_name.cmp(&b.display_name))
            .then_with(|| a.model.cmp(&b.model))
    });

    Ok(FactoryCustomModelsState {
        factory_settings_path,
        session_default_model: default_model,
        models,
    })
}

pub fn list_factory_custom_models() -> Result<FactoryCustomModelsState, String> {
    let path = factory_settings_path()?;
    list_factory_custom_models_at_path(&path)
}

fn remove_factory_custom_models_at_path(
    path: &Path,
    ids: Vec<String>,
) -> Result<FactoryCustomModelsRemoveResult, String> {
    let factory_settings_path = path.to_string_lossy().to_string();

    let mut id_set: HashSet<String> = HashSet::new();
    for id in ids {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            continue;
        }
        id_set.insert(trimmed.to_string());
    }

    if id_set.is_empty() {
        return Ok(FactoryCustomModelsRemoveResult {
            removed: 0,
            skipped_non_proxy: 0,
            skipped_not_found: 0,
            factory_settings_path,
        });
    }

    if !path.exists() {
        return Ok(FactoryCustomModelsRemoveResult {
            removed: 0,
            skipped_non_proxy: 0,
            skipped_not_found: id_set.len(),
            factory_settings_path,
        });
    }

    let mut root = read_json_file(path)?;
    let default_model = session_default_model_id(&root);
    if let Some(default_id) = default_model.as_deref() {
        if id_set.contains(default_id) {
            return Err(format!(
                "Refusing to remove session default model '{}'. Change it in Factory first.",
                default_id
            ));
        }
    }

    let obj = root
        .as_object_mut()
        .ok_or("Factory settings root must be a JSON object")?;

    let existing = obj
        .get("customModels")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut found: HashSet<String> = HashSet::new();
    let mut removed = 0usize;
    let mut skipped_non_proxy = 0usize;
    let mut next = Vec::with_capacity(existing.len());
    for entry in existing {
        let entry_id = entry.get("id").and_then(|v| v.as_str()).map(|s| s.trim());

        if let Some(entry_id) = entry_id {
            if id_set.contains(entry_id) {
                found.insert(entry_id.to_string());
                let base_url = entry.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("");
                if is_proxy_base_url(base_url) {
                    removed += 1;
                    continue;
                }
                skipped_non_proxy += 1;
            }
        }

        next.push(entry);
    }

    let skipped_not_found = id_set.len().saturating_sub(found.len());

    if removed > 0 {
        obj.insert("customModels".to_string(), Value::Array(next));
        write_json_atomic(path, &root, true)?;
    }

    Ok(FactoryCustomModelsRemoveResult {
        removed,
        skipped_non_proxy,
        skipped_not_found,
        factory_settings_path,
    })
}

pub fn remove_factory_custom_models(
    ids: Vec<String>,
) -> Result<FactoryCustomModelsRemoveResult, String> {
    let path = factory_settings_path()?;
    remove_factory_custom_models_at_path(&path, ids)
}

fn update_factory_custom_model_at_path(
    path: &Path,
    id: &str,
    model: Option<String>,
    base_url: Option<String>,
    display_name: Option<String>,
    no_image_support: Option<bool>,
    provider: Option<String>,
) -> Result<FactoryCustomModelRow, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("id is required".to_string());
    }
    if !path.exists() {
        return Err(format!(
            "Factory settings.json not found: {}",
            path.to_string_lossy()
        ));
    }

    let mut root = read_json_file(path)?;
    let default_model = session_default_model_id(&root);
    let default_ref = default_model.as_deref();

    let (changed, updated_entry) = {
        let obj = root
            .as_object_mut()
            .ok_or("Factory settings root must be a JSON object")?;
        let Some(arr) = obj.get_mut("customModels").and_then(|v| v.as_array_mut()) else {
            return Err("Factory settings must contain a 'customModels' array".to_string());
        };

        let mut target_index: Option<usize> = None;
        for (idx, entry) in arr.iter().enumerate() {
            let Some(entry_id) = entry.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            if entry_id.trim() == id {
                target_index = Some(idx);
                break;
            }
        }
        let idx = target_index.ok_or_else(|| format!("Custom model not found: {}", id))?;

        let Some(entry_obj) = arr[idx].as_object_mut() else {
            return Err("Custom model entry must be a JSON object".to_string());
        };

        let current_base_url = entry_obj
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !is_proxy_base_url(current_base_url) {
            return Err("Refusing to edit a non-proxy model".to_string());
        }

        if let Some(ref next_base_url) = base_url {
            let next_trimmed = next_base_url.trim();
            if next_trimmed.is_empty() {
                return Err("baseUrl cannot be empty".to_string());
            }
            if !is_proxy_base_url(next_trimmed) {
                return Err(
                    "Refusing to set baseUrl to a non-proxy endpoint (must be localhost:8317)"
                        .to_string(),
                );
            }
        }

        let mut changed = false;

        if let Some(next_model) = model {
            let next = next_model.trim();
            if next.is_empty() {
                return Err("model cannot be empty".to_string());
            }
            let cur = entry_obj
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if cur != next {
                entry_obj.insert("model".to_string(), Value::String(next.to_string()));
                changed = true;
            }
        }

        if let Some(next_base_url) = base_url {
            let next = next_base_url.trim();
            let cur = entry_obj
                .get("baseUrl")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if cur != next {
                entry_obj.insert("baseUrl".to_string(), Value::String(next.to_string()));
                changed = true;
            }
        }

        if let Some(next_display_name) = display_name {
            let next = next_display_name.trim();
            if next.is_empty() {
                return Err("displayName cannot be empty".to_string());
            }
            let cur = entry_obj
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if cur != next {
                entry_obj.insert("displayName".to_string(), Value::String(next.to_string()));
                changed = true;
            }
        }

        if let Some(next_provider) = provider {
            let next = next_provider.trim();
            if next.is_empty() {
                return Err("provider cannot be empty".to_string());
            }
            let cur = entry_obj
                .get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if cur != next {
                entry_obj.insert("provider".to_string(), Value::String(next.to_string()));
                changed = true;
            }
        }

        if let Some(next_no_image_support) = no_image_support {
            let cur = entry_obj
                .get("noImageSupport")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if cur != next_no_image_support {
                entry_obj.insert(
                    "noImageSupport".to_string(),
                    Value::Bool(next_no_image_support),
                );
                changed = true;
            }
        }

        (changed, arr[idx].clone())
    };

    if changed {
        write_json_atomic(path, &root, true)?;
    }

    parse_custom_model_row(&updated_entry, default_ref)
        .ok_or("Updated custom model could not be parsed".to_string())
}

pub fn update_factory_custom_model(
    id: &str,
    model: Option<String>,
    base_url: Option<String>,
    display_name: Option<String>,
    no_image_support: Option<bool>,
    provider: Option<String>,
) -> Result<FactoryCustomModelRow, String> {
    let path = factory_settings_path()?;
    update_factory_custom_model_at_path(
        &path,
        id,
        model,
        base_url,
        display_name,
        no_image_support,
        provider,
    )
}

fn install_agent_models_at_path(
    path: &Path,
    agent_key: &str,
    models: Vec<FactoryCustomModelInput>,
) -> Result<AgentInstallResult, String> {
    let agent_key = agent_key.trim().to_ascii_lowercase();
    if agent_key.is_empty() {
        return Err("agent_key is required".to_string());
    }

    let mut root = if path.exists() {
        read_json_file(path)?
    } else {
        Value::Object(Default::default())
    };

    let obj = root
        .as_object_mut()
        .ok_or("Factory settings root must be a JSON object")?;

    let mut existing = obj
        .get("customModels")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut existing_keys = existing_custom_model_keys(&existing);
    let mut existing_ids = existing_custom_model_ids(&existing);
    let mut index = next_custom_model_index(&existing);

    let id_prefix = agent_id_prefix(&agent_key);
    let mut added = 0usize;
    let mut skipped_duplicates = 0usize;
    let mut skipped_invalid = 0usize;

    let total_requested = models.len();

    for input in models {
        let model = input.model.trim();
        let base_url = input.base_url.trim();
        let provider = input.provider.trim();
        let display_name = input.display_name.trim();

        if model.is_empty()
            || base_url.is_empty()
            || provider.is_empty()
            || display_name.is_empty()
            || !is_proxy_base_url(base_url)
        {
            skipped_invalid += 1;
            continue;
        }

        let key = model_dedup_key(model, base_url, provider);
        if existing_keys.contains(&key) {
            skipped_duplicates += 1;
            continue;
        }

        let slug_source = format!("{} {}", display_name, model);
        let mut candidate_id = format!("{}{}-{}", id_prefix, slugify(&slug_source), index);
        if existing_ids.contains(&candidate_id) {
            let short = Uuid::new_v4().to_string();
            candidate_id = format!(
                "{}{}-{}-{}",
                id_prefix,
                slugify(&slug_source),
                index,
                &short[..8]
            );
        }
        while existing_ids.contains(&candidate_id) {
            let short = Uuid::new_v4().to_string();
            candidate_id = format!(
                "{}{}-{}-{}",
                id_prefix,
                slugify(&slug_source),
                index,
                &short[..8]
            );
        }

        let id_for_entry = candidate_id.clone();
        let entry = serde_json::json!({
            "model": model,
            "id": id_for_entry,
            "index": index,
            "baseUrl": base_url,
            "apiKey": input.api_key,
            "displayName": display_name,
            "noImageSupport": input.no_image_support,
            "provider": provider
        });

        existing.push(entry);
        existing_keys.insert(key);
        existing_ids.insert(candidate_id);
        index += 1;
        added += 1;
    }

    if added > 0 {
        obj.insert("customModels".to_string(), Value::Array(existing));
        write_json_atomic(path, &root, true)?;
    }

    Ok(AgentInstallResult {
        agent_key,
        total_requested,
        added,
        skipped_duplicates,
        skipped_invalid,
        factory_settings_path: path.to_string_lossy().to_string(),
    })
}

pub fn install_agent_models(
    agent_key: &str,
    models: Vec<FactoryCustomModelInput>,
) -> Result<AgentInstallResult, String> {
    let path = factory_settings_path()?;
    install_agent_models_at_path(&path, agent_key, models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_temp_settings_path() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "codeforwarder-factory-settings-test-{}",
            Uuid::new_v4().to_string()
        ));
        root.join(".factory").join("settings.json")
    }

    #[test]
    fn install_dedups_and_generates_ids() {
        let path = make_temp_settings_path();
        ensure_parent_dir(&path).unwrap();

        let existing = serde_json::json!({
            "customModels": [
                {
                    "model": "gpt-4.1",
                    "id": "custom:droid:existing-0",
                    "index": 0,
                    "baseUrl": "http://localhost:8317/v1",
                    "apiKey": "dummy",
                    "displayName": "GPT 4.1",
                    "noImageSupport": false,
                    "provider": "openai"
                }
            ]
        });
        fs::write(&path, serde_json::to_vec_pretty(&existing).unwrap()).unwrap();

        let models = vec![
            FactoryCustomModelInput {
                model: "gpt-4.1".to_string(),
                base_url: "http://localhost:8317/v1".to_string(),
                api_key: "dummy-not-used".to_string(),
                display_name: "GPT 4.1".to_string(),
                no_image_support: false,
                provider: "openai".to_string(),
            },
            FactoryCustomModelInput {
                model: "claude-3-5-sonnet".to_string(),
                base_url: "http://localhost:8317".to_string(),
                api_key: "dummy-not-used".to_string(),
                display_name: "Sonnet".to_string(),
                no_image_support: true,
                provider: "anthropic".to_string(),
            },
        ];

        let res = install_agent_models_at_path(&path, "droid", models).unwrap();
        assert_eq!(res.total_requested, 2);
        assert_eq!(res.added, 1);
        assert_eq!(res.skipped_duplicates, 1);
        assert_eq!(res.skipped_invalid, 0);
        assert!(res.factory_settings_path.ends_with("settings.json"));

        let root = read_json_file(&path).unwrap();
        let arr = root
            .get("customModels")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(arr.len(), 2);

        let added_entry = arr
            .iter()
            .find(|v| v.get("model").and_then(|m| m.as_str()) == Some("claude-3-5-sonnet"))
            .unwrap();
        let id = added_entry.get("id").and_then(|v| v.as_str()).unwrap();
        assert!(id.starts_with("custom:droid:"));
        assert_eq!(added_entry.get("index").and_then(|v| v.as_i64()), Some(1));

        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn list_includes_proxy_and_external_models() {
        let path = make_temp_settings_path();
        ensure_parent_dir(&path).unwrap();

        let settings = serde_json::json!({
            "customModels": [
                {"id": "custom:proxy-0", "model": "gpt-4.1", "index": 0, "baseUrl": "http://localhost:8317/v1", "apiKey": "dummy", "displayName": "Proxy", "noImageSupport": false, "provider": "openai"},
                {"id": "custom:external-1", "model": "kimi-k2.5", "index": 1, "baseUrl": "https://opencode.ai/zen/v1", "apiKey": "sk-REDACTED", "displayName": "External", "noImageSupport": false, "provider": "generic-chat-completion-api"}
            ],
            "sessionDefaultSettings": {"model": "custom:proxy-0"}
        });
        fs::write(&path, serde_json::to_vec_pretty(&settings).unwrap()).unwrap();

        let state = list_factory_custom_models_at_path(&path).unwrap();
        assert_eq!(state.models.len(), 2);
        assert_eq!(
            state.session_default_model.as_deref(),
            Some("custom:proxy-0")
        );
        assert!(state.models[0].is_proxy);
        assert!(state.models[0].is_session_default);
        assert!(!state.models[1].is_proxy);
        assert!(!state.models[1].is_session_default);

        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn remove_skips_non_proxy_and_refuses_default() {
        let path = make_temp_settings_path();
        ensure_parent_dir(&path).unwrap();

        let settings = serde_json::json!({
            "customModels": [
                {"id": "custom:proxy-0", "model": "gpt-4.1", "index": 0, "baseUrl": "http://localhost:8317/v1", "apiKey": "dummy", "displayName": "Proxy", "noImageSupport": false, "provider": "openai"},
                {"id": "custom:external-1", "model": "kimi-k2.5", "index": 1, "baseUrl": "https://opencode.ai/zen/v1", "apiKey": "sk-REDACTED", "displayName": "External", "noImageSupport": false, "provider": "generic-chat-completion-api"}
            ],
            "sessionDefaultSettings": {"model": "custom:proxy-0"}
        });
        fs::write(&path, serde_json::to_vec_pretty(&settings).unwrap()).unwrap();

        let err = remove_factory_custom_models_at_path(&path, vec!["custom:proxy-0".to_string()])
            .unwrap_err();
        assert!(err.contains("session default"));

        let res =
            remove_factory_custom_models_at_path(&path, vec!["custom:external-1".to_string()])
                .unwrap();
        assert_eq!(res.removed, 0);
        assert_eq!(res.skipped_non_proxy, 1);

        // Now allow removing the proxy by clearing the default.
        let settings2 = serde_json::json!({
            "customModels": [
                {"id": "custom:proxy-0", "model": "gpt-4.1", "index": 0, "baseUrl": "http://localhost:8317/v1", "apiKey": "dummy", "displayName": "Proxy", "noImageSupport": false, "provider": "openai"},
                {"id": "custom:external-1", "model": "kimi-k2.5", "index": 1, "baseUrl": "https://opencode.ai/zen/v1", "apiKey": "sk-REDACTED", "displayName": "External", "noImageSupport": false, "provider": "generic-chat-completion-api"}
            ]
        });
        fs::write(&path, serde_json::to_vec_pretty(&settings2).unwrap()).unwrap();

        let res2 = remove_factory_custom_models_at_path(
            &path,
            vec![
                "custom:proxy-0".to_string(),
                "custom:external-1".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(res2.removed, 1);
        assert_eq!(res2.skipped_non_proxy, 1);
        assert_eq!(res2.skipped_not_found, 0);

        let root = read_json_file(&path).unwrap();
        let arr = root
            .get("customModels")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(arr.len(), 1);
        assert_eq!(
            arr[0].get("id").and_then(|v| v.as_str()),
            Some("custom:external-1")
        );

        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn update_refuses_non_proxy_and_updates_proxy() {
        let path = make_temp_settings_path();
        ensure_parent_dir(&path).unwrap();

        let settings = serde_json::json!({
            "customModels": [
                {"id": "custom:proxy-0", "model": "gpt-4.1", "index": 0, "baseUrl": "http://localhost:8317/v1", "apiKey": "dummy", "displayName": "Proxy", "noImageSupport": false, "provider": "openai"},
                {"id": "custom:external-1", "model": "kimi-k2.5", "index": 1, "baseUrl": "https://opencode.ai/zen/v1", "apiKey": "sk-REDACTED", "displayName": "External", "noImageSupport": false, "provider": "generic-chat-completion-api"}
            ]
        });
        fs::write(&path, serde_json::to_vec_pretty(&settings).unwrap()).unwrap();

        let err = update_factory_custom_model_at_path(
            &path,
            "custom:external-1",
            None,
            None,
            Some("New".to_string()),
            None,
            None,
        )
        .unwrap_err();
        assert!(err.contains("non-proxy"));

        let updated = update_factory_custom_model_at_path(
            &path,
            "custom:proxy-0",
            None,
            None,
            Some("Proxy Updated".to_string()),
            Some(true),
            Some("openai".to_string()),
        )
        .unwrap();
        assert_eq!(updated.display_name, "Proxy Updated");
        assert!(updated.no_image_support);
        assert!(updated.is_proxy);

        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }
}
