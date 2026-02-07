use crate::auth_manager;
use crate::binary_manager;
use crate::config_manager;
use crate::server_manager::ServerManager;
use crate::settings;
use crate::thinking_proxy::ThinkingProxy;
use crate::tray;
use crate::types::*;
use crate::usage_tracker::{UsageRangeQuery, UsageTracker};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Emitter, State};
use tauri_plugin_autostart::ManagerExt as AutoStartManagerExt;
use tokio::sync::{Mutex, RwLock};

pub struct AppState {
    pub server_manager: Arc<RwLock<ServerManager>>,
    pub thinking_proxy: Arc<RwLock<ThinkingProxy>>,
    pub lifecycle_lock: Arc<Mutex<()>>,
    pub binary_downloading: Arc<AtomicBool>,
    pub usage_tracker: Arc<UsageTracker>,
}

async fn run_blocking<F, T>(job: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|e| format!("Failed to join blocking task: {}", e))?
}

#[tauri::command]
pub async fn get_server_state(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<ServerState, String> {
    let mut sm = state.server_manager.write().await;
    sm.refresh_running_status().await;
    let tp = state.thinking_proxy.read().await;
    Ok(ServerState {
        is_running: sm.is_running() && tp.is_running(),
        proxy_port: 8317,
        backend_port: 8318,
        binary_available: binary_manager::is_binary_available_for_app(&app),
        binary_downloading: state.binary_downloading.load(Ordering::Relaxed),
    })
}

#[tauri::command]
pub async fn start_server(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let _lifecycle_guard = state.lifecycle_lock.lock().await;

    let app_for_binary = app.clone();
    let binary_path =
        run_blocking(move || binary_manager::ensure_binary_installed(&app_for_binary)).await?;

    let settings = settings::load_settings(&app);
    let app_for_config = app.clone();
    let enabled_providers = settings.enabled_providers.clone();
    let config_path = run_blocking(move || {
        config_manager::get_merged_config_path(&app_for_config, &enabled_providers)
    })
    .await?;
    let config_path_str = config_path.to_string_lossy().to_string();
    let binary_path_str = binary_path.to_string_lossy().to_string();

    // Always perform a clean restart so stale background processes cannot block startup.
    {
        let mut tp = state.thinking_proxy.write().await;
        tp.stop().await;
    }
    {
        let mut sm = state.server_manager.write().await;
        sm.stop().await;
    }
    ServerManager::kill_orphaned_processes().await;
    ServerManager::cleanup_port_conflicts_for_restart().await?;

    // Start thinking proxy first
    {
        let mut tp = state.thinking_proxy.write().await;
        tp.start()
            .await
            .map_err(|e| format!("Failed to start thinking proxy: {}", e))?;
    }

    // Then start the backend server
    {
        let mut sm = state.server_manager.write().await;
        sm.start(&config_path_str, &binary_path_str).await?;
    }

    // Update tray state
    tray::update_tray_state(&app, true);

    // Emit status change
    let server_state = ServerState {
        is_running: true,
        proxy_port: 8317,
        backend_port: 8318,
        binary_available: true,
        binary_downloading: false,
    };
    app.emit("server_status_changed", &server_state).ok();

    Ok(())
}

#[tauri::command]
pub async fn stop_server(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let _lifecycle_guard = state.lifecycle_lock.lock().await;

    // Stop thinking proxy first
    {
        let mut tp = state.thinking_proxy.write().await;
        tp.stop().await;
    }

    // Then stop backend
    {
        let mut sm = state.server_manager.write().await;
        sm.stop().await;
    }

    // Update tray state
    tray::update_tray_state(&app, false);

    // Emit status change
    let server_state = ServerState {
        is_running: false,
        proxy_port: 8317,
        backend_port: 8318,
        binary_available: binary_manager::is_binary_available_for_app(&app),
        binary_downloading: false,
    };
    app.emit("server_status_changed", &server_state).ok();

    Ok(())
}

#[tauri::command]
pub async fn get_auth_accounts() -> Result<HashMap<String, ServiceAccounts>, String> {
    let accounts = tokio::task::spawn_blocking(auth_manager::scan_auth_directory)
        .await
        .map_err(|e| format!("Failed to join auth scan task: {}", e))?;

    let mut result = HashMap::new();
    for (st, sa) in accounts {
        result.insert(st.provider_key().to_string(), sa);
    }
    Ok(result)
}

#[tauri::command]
pub async fn run_auth(
    app: tauri::AppHandle,
    command: AuthCommand,
) -> Result<(bool, String), String> {
    let app_for_binary = app.clone();
    let binary_path =
        run_blocking(move || binary_manager::ensure_binary_installed(&app_for_binary)).await?;

    let settings = settings::load_settings(&app);
    let app_for_config = app.clone();
    let enabled_providers = settings.enabled_providers.clone();
    let config_path = run_blocking(move || {
        config_manager::get_merged_config_path(&app_for_config, &enabled_providers)
    })
    .await?;
    let config_path_str = config_path.to_string_lossy().to_string();
    let binary_path_str = binary_path.to_string_lossy().to_string();

    ServerManager::run_auth_command(&binary_path_str, &config_path_str, &command).await
}

#[tauri::command]
pub async fn delete_auth_account(file_path: String) -> Result<bool, String> {
    run_blocking(move || {
        auth_manager::delete_account(&file_path)?;
        Ok(true)
    })
    .await
}

#[tauri::command]
pub async fn save_zai_api_key(api_key: String) -> Result<(bool, String), String> {
    run_blocking(move || ServerManager::save_zai_api_key(&api_key)).await
}

#[tauri::command]
pub fn get_settings(app: tauri::AppHandle) -> Result<AppSettings, String> {
    let mut current = settings::load_settings(&app);
    if let Ok(is_enabled) = app.autolaunch().is_enabled() {
        if current.launch_at_login != is_enabled {
            current.launch_at_login = is_enabled;
            if let Err(e) = settings::save_settings(&app, &current) {
                log::warn!("[Settings] Failed to sync launch_at_login state: {}", e);
            }
        }
    }
    Ok(current)
}

#[tauri::command]
pub async fn set_provider_enabled(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    provider: String,
    enabled: bool,
) -> Result<(), String> {
    let mut current = settings::load_settings(&app);
    current.enabled_providers.insert(provider.clone(), enabled);
    settings::save_settings(&app, &current)?;

    // Regenerate config (hot reload)
    let app_for_config = app.clone();
    let enabled_providers = current.enabled_providers.clone();
    run_blocking(move || {
        config_manager::get_merged_config_path(&app_for_config, &enabled_providers).map(|_| ())
    })
    .await?;

    // Update thinking proxy vercel config if needed
    let vercel_config_handle = {
        let tp = state.thinking_proxy.read().await;
        tp.vercel_config.clone()
    };
    {
        let mut vc = vercel_config_handle.write().await;
        *vc = VercelGatewayConfig {
            enabled: current.vercel_gateway_enabled,
            api_key: current.vercel_api_key.clone(),
        };
    }

    Ok(())
}

#[tauri::command]
pub async fn set_vercel_config(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
    api_key: String,
) -> Result<(), String> {
    let mut current = settings::load_settings(&app);
    current.vercel_gateway_enabled = enabled;
    current.vercel_api_key = api_key.clone();
    settings::save_settings(&app, &current)?;

    // Update thinking proxy
    let vercel_config_handle = {
        let tp = state.thinking_proxy.read().await;
        tp.vercel_config.clone()
    };
    {
        let mut vc = vercel_config_handle.write().await;
        *vc = VercelGatewayConfig { enabled, api_key };
    }

    Ok(())
}

#[tauri::command]
pub fn set_launch_at_login(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        app.autolaunch()
            .enable()
            .map_err(|e| format!("Failed to enable launch at login: {}", e))?;
    } else {
        app.autolaunch()
            .disable()
            .map_err(|e| format!("Failed to disable launch at login: {}", e))?;
    }

    let mut current = settings::load_settings(&app);
    current.launch_at_login = enabled;
    settings::save_settings(&app, &current)?;

    Ok(())
}

#[tauri::command]
pub fn check_binary(app: tauri::AppHandle) -> Result<bool, String> {
    Ok(binary_manager::is_binary_available_for_app(&app))
}

#[tauri::command]
pub async fn download_binary(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    state.binary_downloading.store(true, Ordering::SeqCst);

    let is_running = {
        let mut sm = state.server_manager.write().await;
        sm.refresh_running_status().await;
        let tp = state.thinking_proxy.read().await;
        sm.is_running() && tp.is_running()
    };
    app.emit(
        "server_status_changed",
        ServerState {
            is_running,
            proxy_port: 8317,
            backend_port: 8318,
            binary_available: binary_manager::is_binary_available_for_app(&app),
            binary_downloading: true,
        },
    )
    .ok();

    let release = binary_manager::get_latest_release_info().await;
    let result = match release {
        Ok(release) => binary_manager::download_binary(app.clone(), &release).await,
        Err(e) => Err(e),
    };

    state.binary_downloading.store(false, Ordering::SeqCst);

    let is_running = {
        let mut sm = state.server_manager.write().await;
        sm.refresh_running_status().await;
        let tp = state.thinking_proxy.read().await;
        sm.is_running() && tp.is_running()
    };
    let binary_available = result
        .as_ref()
        .map(|_| true)
        .unwrap_or_else(|_| binary_manager::is_binary_available_for_app(&app));
    app.emit(
        "server_status_changed",
        ServerState {
            is_running,
            proxy_port: 8317,
            backend_port: 8318,
            binary_available,
            binary_downloading: false,
        },
    )
    .ok();

    result
}

#[tauri::command]
pub async fn open_auth_folder() -> Result<(), String> {
    run_blocking(|| {
        let auth_dir = auth_manager::get_auth_dir();
        open::that(&auth_dir).map_err(|e| format!("Failed to open auth folder: {}", e))
    })
    .await
}

#[tauri::command]
pub fn copy_server_url() -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;
    clipboard
        .set_text("http://localhost:8317")
        .map_err(|e| format!("Failed to copy to clipboard: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn sync_theme_icons(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    is_dark: bool,
) -> Result<(), String> {
    let theme = if is_dark {
        tray::TrayTheme::Dark
    } else {
        tray::TrayTheme::Light
    };
    tray::set_theme_override(&app, Some(theme));
    tray::update_main_window_icon(&app);

    let is_running = {
        let mut sm = state.server_manager.write().await;
        sm.refresh_running_status().await;
        let tp = state.thinking_proxy.read().await;
        sm.is_running() && tp.is_running()
    };
    tray::update_tray_state(&app, is_running);

    Ok(())
}

#[tauri::command]
pub async fn get_usage_dashboard(
    state: State<'_, AppState>,
    range: Option<String>,
) -> Result<UsageDashboardPayload, String> {
    let range = range.unwrap_or_else(|| "7d".to_string());
    let parsed_range = UsageRangeQuery::from_input(&range);
    let vibe = state.usage_tracker.get_vibe_dashboard(parsed_range).await?;
    Ok(UsageDashboardPayload { vibe })
}
