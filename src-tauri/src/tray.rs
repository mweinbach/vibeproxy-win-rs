use std::sync::Mutex;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager,
};

#[cfg(not(target_os = "macos"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(target_os = "windows")]
fn apply_hidden_process_flags(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// Store menu item references for later updates
pub struct TrayMenuItems {
    pub status: MenuItem<tauri::Wry>,
    pub start_stop: MenuItem<tauri::Wry>,
    pub copy_url: MenuItem<tauri::Wry>,
}

pub struct TrayThemeState(pub Mutex<Option<TrayTheme>>);

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let status_item = MenuItem::with_id(app, "status", "Server: Stopped", false, None::<&str>)?;
    let separator1 = PredefinedMenuItem::separator(app)?;
    let open_settings =
        MenuItem::with_id(app, "open_settings", "Open Settings", true, None::<&str>)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let start_stop = MenuItem::with_id(app, "start_stop", "Start Server", true, None::<&str>)?;
    let separator3 = PredefinedMenuItem::separator(app)?;
    let copy_url = MenuItem::with_id(app, "copy_url", "Copy Server URL", false, None::<&str>)?;
    let separator4 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &status_item,
            &separator1,
            &open_settings,
            &separator2,
            &start_stop,
            &separator3,
            &copy_url,
            &separator4,
            &quit,
        ],
    )?;

    // Store menu items for later updates
    app.manage(Mutex::new(TrayMenuItems {
        status: status_item,
        start_stop,
        copy_url,
    }));
    app.manage(TrayThemeState(Mutex::new(None)));

    let icon = load_tray_icon(app, false);

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        // macOS: treat the icon as a template so the system automatically tints it
        // (light/dark menu bar, vibrancy, etc).
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("CodeForwarder")
        .menu(&menu)
        // macOS status-bar icons conventionally show the menu on left click.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(move |app, event| {
            handle_menu_event(app, event.id().as_ref());
        })
        .on_tray_icon_event(|_tray, _event| {
            #[cfg(not(target_os = "macos"))]
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = _event
            {
                let app = _tray.app_handle();
                show_main_window(app);
            }
        })
        .build(app)?;

    Ok(())
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "open_settings" => {
            show_main_window(app);
        }
        "start_stop" => {
            app.emit("tray_start_stop_clicked", ()).ok();
        }
        "copy_url" => {
            app.emit("tray_copy_url_clicked", ()).ok();
        }
        "quit" => {
            app.emit("tray_quit_clicked", ()).ok();
            let app_handle = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(500));
                app_handle.exit(0);
            });
        }
        _ => {}
    }
}

fn show_main_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        // If the app is running as a UIElement (no Dock icon), bring it back
        // when showing the main window.
        app.set_dock_visibility(true).ok();
    }

    if let Some(window) = app.get_webview_window("main") {
        window.show().ok();
        window.unminimize().ok();
        window.set_focus().ok();
    }
}

#[derive(Clone, Copy)]
pub enum TrayTheme {
    Light,
    Dark,
}

pub fn set_theme_override(app: &AppHandle, theme: Option<TrayTheme>) {
    if let Some(state) = app.try_state::<TrayThemeState>() {
        if let Ok(mut value) = state.0.lock() {
            *value = theme;
        }
    }
}

fn current_theme(app: &AppHandle) -> TrayTheme {
    if let Some(state) = app.try_state::<TrayThemeState>() {
        if let Ok(value) = state.0.lock() {
            if let Some(theme) = *value {
                return theme;
            }
        }
    }

    detect_taskbar_theme()
}

fn detect_taskbar_theme() -> TrayTheme {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = std::process::Command::new("reg");
        apply_hidden_process_flags(&mut cmd);
        let query = cmd
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
                "/v",
                "SystemUsesLightTheme",
            ])
            .output();

        if let Ok(output) = query {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.contains("0x1") {
                    return TrayTheme::Light;
                }
                if stdout.contains("0x0") {
                    return TrayTheme::Dark;
                }
            }
        }
    }

    TrayTheme::Light
}

fn themed_icon_name(active: bool, theme: TrayTheme) -> &'static str {
    match (active, theme) {
        (true, TrayTheme::Light) => "icon-active-light.png",
        (true, TrayTheme::Dark) => "icon-active-dark.png",
        (false, TrayTheme::Light) => "icon-inactive-light.png",
        (false, TrayTheme::Dark) => "icon-inactive-dark.png",
    }
}

fn fallback_icon_bytes(active: bool, theme: TrayTheme) -> &'static [u8] {
    match (active, theme) {
        (true, TrayTheme::Light) => include_bytes!("../resources/icon-active-light.png"),
        (true, TrayTheme::Dark) => include_bytes!("../resources/icon-active-dark.png"),
        (false, TrayTheme::Light) => include_bytes!("../resources/icon-inactive-light.png"),
        (false, TrayTheme::Dark) => include_bytes!("../resources/icon-inactive-dark.png"),
    }
}

fn load_image_from_resources(app: &AppHandle, file_name: &str) -> Option<Image<'static>> {
    let resource_dir = app.path().resource_dir().ok()?;
    let direct_path = resource_dir.join(file_name);
    if let Ok(image) = Image::from_path(&direct_path) {
        return Some(image);
    }

    let nested_path = resource_dir.join("resources").join(file_name);
    Image::from_path(nested_path).ok()
}

fn load_tray_icon(app: &AppHandle, active: bool) -> Image<'static> {
    let theme = current_theme(app);
    let icon_name = themed_icon_name(active, theme);

    if let Some(icon) = load_image_from_resources(app, icon_name) {
        return icon;
    }

    let fallback_name = if active {
        "icon-active.png"
    } else {
        "icon-inactive.png"
    };
    if let Some(icon) = load_image_from_resources(app, fallback_name) {
        return icon;
    }

    // Fallback: use included bytes
    Image::from_bytes(fallback_icon_bytes(active, theme))
        .or_else(|_| {
            let fallback_bytes: &[u8] = if active {
                &include_bytes!("../resources/icon-active.png")[..]
            } else {
                &include_bytes!("../resources/icon-inactive.png")[..]
            };
            Image::from_bytes(fallback_bytes)
        })
        .expect("Failed to load fallback tray icon")
}

fn themed_window_icon_name(theme: TrayTheme) -> &'static str {
    match theme {
        TrayTheme::Light => "icon-active-light.png",
        TrayTheme::Dark => "icon-active-dark.png",
    }
}

fn fallback_window_icon_bytes(theme: TrayTheme) -> &'static [u8] {
    match theme {
        TrayTheme::Light => include_bytes!("../resources/icon-active-light.png"),
        TrayTheme::Dark => include_bytes!("../resources/icon-active-dark.png"),
    }
}

fn load_window_icon(app: &AppHandle, theme: TrayTheme) -> Image<'static> {
    let icon_name = themed_window_icon_name(theme);
    if let Some(icon) = load_image_from_resources(app, icon_name) {
        return icon;
    }

    Image::from_bytes(fallback_window_icon_bytes(theme))
        .expect("Failed to load fallback window icon")
}

pub fn update_main_window_icon(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let icon = load_window_icon(app, current_theme(app));
        window.set_icon(icon).ok();
    }
}

pub fn update_tray_state(app: &AppHandle, is_running: bool) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        // Update icon
        let icon = load_tray_icon(app, is_running);
        tray.set_icon(Some(icon)).ok();
        #[cfg(target_os = "macos")]
        {
            // Re-apply template mode after icon changes.
            tray.set_icon_as_template(true).ok();
        }

        // Update tooltip
        let tooltip = if is_running {
            "CodeForwarder - Running (port 8317)"
        } else {
            "CodeForwarder - Stopped"
        };
        tray.set_tooltip(Some(tooltip)).ok();
    }

    // Update menu items via stored references
    if let Ok(items) = app.state::<Mutex<TrayMenuItems>>().lock() {
        let status_text = if is_running {
            "Server: Running (port 8317)"
        } else {
            "Server: Stopped"
        };
        items.status.set_text(status_text).ok();

        let action_text = if is_running {
            "Stop Server"
        } else {
            "Start Server"
        };
        items.start_stop.set_text(action_text).ok();
        items.copy_url.set_enabled(is_running).ok();
    }
}
