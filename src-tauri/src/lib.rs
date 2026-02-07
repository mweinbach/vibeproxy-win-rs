mod auth_manager;
mod binary_manager;
mod commands;
mod config_manager;
mod managed_key;
mod secure_store;
mod server_manager;
mod settings;
mod thinking_proxy;
mod tray;
mod types;
mod usage_tracker;

use commands::AppState;
use server_manager::ServerManager;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::{Listener, Manager};
use tauri_plugin_autostart::ManagerExt as AutoStartManagerExt;
use thinking_proxy::ThinkingProxy;
use tokio::sync::{Mutex, RwLock};
use types::VercelGatewayConfig;
use usage_tracker::UsageTracker;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                window.show().ok();
                window.unminimize().ok();
                window.set_focus().ok();
            }
        }))
        .invoke_handler(tauri::generate_handler![
            commands::get_server_state,
            commands::start_server,
            commands::stop_server,
            commands::get_auth_accounts,
            commands::run_auth,
            commands::delete_auth_account,
            commands::save_zai_api_key,
            commands::get_settings,
            commands::set_provider_enabled,
            commands::set_vercel_config,
            commands::set_launch_at_login,
            commands::check_binary,
            commands::download_binary,
            commands::open_auth_folder,
            commands::copy_server_url,
            commands::sync_theme_icons,
            commands::get_usage_dashboard,
        ])
        .setup(|app| {
            let app_handle = app.handle().clone();

            // Load settings
            let app_settings = settings::load_settings(&app_handle);
            if app_settings.launch_at_login {
                if let Err(e) = app_handle.autolaunch().enable() {
                    log::error!("[Setup] Failed to enable launch at login: {}", e);
                }
            } else if let Err(e) = app_handle.autolaunch().disable() {
                log::error!("[Setup] Failed to disable launch at login: {}", e);
            }

            // Create shared vercel config
            let vercel_config = Arc::new(RwLock::new(VercelGatewayConfig {
                enabled: app_settings.vercel_gateway_enabled,
                api_key: app_settings.vercel_api_key.clone(),
            }));

            // Create managers
            let server_manager = Arc::new(RwLock::new(ServerManager::new()));
            let usage_tracker = match UsageTracker::new() {
                Ok(tracker) => Arc::new(tracker),
                Err(e) => {
                    log::error!("[Setup] Failed to initialize usage tracker: {}", e);
                    return Err(Box::new(std::io::Error::other(e)));
                }
            };
            let thinking_proxy = Arc::new(RwLock::new(ThinkingProxy::new(
                vercel_config,
                usage_tracker.clone(),
            )));
            let lifecycle_lock = Arc::new(Mutex::new(()));
            let binary_downloading = Arc::new(AtomicBool::new(false));

            // Register app state
            app.manage(AppState {
                server_manager: server_manager.clone(),
                thinking_proxy: thinking_proxy.clone(),
                lifecycle_lock: lifecycle_lock.clone(),
                binary_downloading: binary_downloading.clone(),
                usage_tracker: usage_tracker.clone(),
            });

            // Setup system tray
            tray::setup_tray(&app_handle)?;
            tray::update_main_window_icon(&app_handle);

            // Ensure auth directory exists
            auth_manager::get_auth_dir();

            // Setup file watcher on auth directory
            let auth_watcher_handle = app_handle.clone();
            std::thread::spawn(move || {
                setup_auth_watcher(auth_watcher_handle);
            });

            // Auto-start server if binary is available
            let auto_start_handle = app_handle.clone();
            let sm = server_manager.clone();
            let tp = thinking_proxy.clone();
            let startup_lifecycle_lock = lifecycle_lock.clone();
            tauri::async_runtime::spawn(async move {
                let _lifecycle_guard = startup_lifecycle_lock.lock().await;

                if binary_manager::is_binary_available_for_app(&auto_start_handle) {
                    log::info!("[Setup] Binary available, auto-starting server...");

                    let app_settings = settings::load_settings(&auto_start_handle);
                    let config_path = build_merged_config_path(
                        auto_start_handle.clone(),
                        app_settings.enabled_providers.clone(),
                    )
                    .await;

                    match config_path {
                        Ok(config_path) => {
                            let config_path_str = config_path.to_string_lossy().to_string();
                            let binary_path =
                                match build_runtime_binary_path(auto_start_handle.clone()).await {
                                    Ok(path) => path,
                                    Err(e) => {
                                        log::error!(
                                            "[Setup] Failed to locate runtime binary: {}",
                                            e
                                        );
                                        return;
                                    }
                                };
                            let binary_path_str = binary_path.to_string_lossy().to_string();

                            {
                                let mut tp = tp.write().await;
                                tp.stop().await;
                            }
                            {
                                let mut sm = sm.write().await;
                                sm.stop().await;
                            }
                            ServerManager::kill_orphaned_processes().await;
                            if let Err(e) =
                                ServerManager::cleanup_port_conflicts_for_restart().await
                            {
                                log::error!("[Setup] Failed to clear stale listeners: {}", e);
                                return;
                            }

                            // Start thinking proxy
                            {
                                let mut tp = tp.write().await;
                                if let Err(e) = tp.start().await {
                                    log::error!("[Setup] Failed to start thinking proxy: {}", e);
                                    return;
                                }
                            }

                            // Start backend server
                            {
                                let mut sm = sm.write().await;
                                if let Err(e) = sm.start(&config_path_str, &binary_path_str).await {
                                    log::error!("[Setup] Failed to start server: {}", e);
                                    let mut tp = tp.write().await;
                                    tp.stop().await;
                                    return;
                                }
                            }

                            tray::update_tray_state(&auto_start_handle, true);

                            use tauri::Emitter;
                            auto_start_handle
                                .emit(
                                    "server_status_changed",
                                    types::ServerState {
                                        is_running: true,
                                        proxy_port: 8317,
                                        backend_port: 8318,
                                        binary_available: true,
                                        binary_downloading: false,
                                    },
                                )
                                .ok();

                            log::info!("[Setup] Server started successfully");
                        }
                        Err(e) => {
                            log::error!("[Setup] Failed to generate merged config: {}", e);
                        }
                    }
                } else {
                    log::info!("[Setup] Binary not available, skipping auto-start");
                }
            });

            // Handle tray events
            let tray_handle = app_handle.clone();
            let tray_sm = server_manager.clone();
            let tray_tp = thinking_proxy.clone();
            let tray_lifecycle_lock = lifecycle_lock.clone();
            app.listen("tray_start_stop_clicked", move |_| {
                let handle = tray_handle.clone();
                let sm = tray_sm.clone();
                let tp = tray_tp.clone();
                let lifecycle_lock = tray_lifecycle_lock.clone();
                tauri::async_runtime::spawn(async move {
                    let _lifecycle_guard = lifecycle_lock.lock().await;

                    let is_running = {
                        let mut sm = sm.write().await;
                        sm.refresh_running_status().await;
                        sm.is_running()
                    };

                    if is_running {
                        {
                            let mut tp = tp.write().await;
                            tp.stop().await;
                        }
                        {
                            let mut sm = sm.write().await;
                            sm.stop().await;
                        }
                        tray::update_tray_state(&handle, false);
                        use tauri::Emitter;
                        handle
                            .emit(
                                "server_status_changed",
                                types::ServerState {
                                    is_running: false,
                                    proxy_port: 8317,
                                    backend_port: 8318,
                                    binary_available: binary_manager::is_binary_available_for_app(
                                        &handle,
                                    ),
                                    binary_downloading: false,
                                },
                            )
                            .ok();
                    } else {
                        let s = settings::load_settings(&handle);
                        match build_merged_config_path(handle.clone(), s.enabled_providers.clone())
                            .await
                        {
                            Ok(config_path) => {
                                let config_str = config_path.to_string_lossy().to_string();
                                let binary_path =
                                    match build_runtime_binary_path(handle.clone()).await {
                                        Ok(path) => path,
                                        Err(e) => {
                                            log::error!("Failed to locate runtime binary: {}", e);
                                            return;
                                        }
                                    };
                                let bin_str = binary_path.to_string_lossy().to_string();

                                {
                                    let mut tp = tp.write().await;
                                    tp.stop().await;
                                }
                                {
                                    let mut sm = sm.write().await;
                                    sm.stop().await;
                                }
                                ServerManager::kill_orphaned_processes().await;
                                if let Err(e) =
                                    ServerManager::cleanup_port_conflicts_for_restart().await
                                {
                                    log::error!("Failed to clear stale listeners: {}", e);
                                    return;
                                }

                                {
                                    let mut tp = tp.write().await;
                                    if let Err(e) = tp.start().await {
                                        log::error!("Failed to start thinking proxy: {}", e);
                                        return;
                                    }
                                }
                                {
                                    let mut sm = sm.write().await;
                                    if let Err(e) = sm.start(&config_str, &bin_str).await {
                                        log::error!("Failed to start server: {}", e);
                                        let mut tp = tp.write().await;
                                        tp.stop().await;
                                        return;
                                    }
                                }
                                tray::update_tray_state(&handle, true);
                                use tauri::Emitter;
                                handle
                                    .emit(
                                        "server_status_changed",
                                        types::ServerState {
                                            is_running: true,
                                            proxy_port: 8317,
                                            backend_port: 8318,
                                            binary_available: true,
                                            binary_downloading: false,
                                        },
                                    )
                                    .ok();
                            }
                            Err(e) => {
                                log::error!("Failed to generate merged config: {}", e);
                            }
                        }
                    }
                });
            });

            // Handle copy URL from tray
            app.listen("tray_copy_url_clicked", move |_| {
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    clipboard.set_text("http://localhost:8317").ok();
                }
            });

            // Window close -> hide to tray instead of closing
            let close_handle = app_handle.clone();
            if let Some(window) = app.get_webview_window("main") {
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        if let Some(win) = close_handle.get_webview_window("main") {
                            win.hide().ok();
                        }
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

async fn build_merged_config_path(
    app_handle: tauri::AppHandle,
    enabled_providers: std::collections::HashMap<String, bool>,
) -> Result<std::path::PathBuf, String> {
    tokio::task::spawn_blocking(move || {
        config_manager::get_merged_config_path(&app_handle, &enabled_providers)
    })
    .await
    .map_err(|e| format!("Failed to join config generation task: {}", e))?
}

async fn build_runtime_binary_path(
    app_handle: tauri::AppHandle,
) -> Result<std::path::PathBuf, String> {
    tokio::task::spawn_blocking(move || binary_manager::ensure_binary_installed(&app_handle))
        .await
        .map_err(|e| format!("Failed to join binary resolution task: {}", e))?
}

fn setup_auth_watcher(app_handle: tauri::AppHandle) {
    use notify_debouncer_mini::new_debouncer;
    use std::time::Duration;

    let auth_dir = auth_manager::get_auth_dir();

    let handle = app_handle.clone();
    let mut debouncer = new_debouncer(Duration::from_millis(500), move |_res| {
        log::info!("[FileWatcher] Auth directory changed, emitting event");
        use tauri::Emitter;
        handle.emit("auth_accounts_changed", ()).ok();
    })
    .expect("Failed to create file watcher");

    debouncer
        .watcher()
        .watch(&auth_dir, notify::RecursiveMode::NonRecursive)
        .expect("Failed to watch auth directory");

    // Keep the debouncer alive for the lifetime of the app
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}
