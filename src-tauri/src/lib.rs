mod server;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use server::{start_server, AppState, ServerPaths};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_opener::OpenerExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    port: u16,
    display_monitor_index: usize,
    auto_start_display: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            port: 8787,
            display_monitor_index: 0,
            // Keep the control window visible on first launch.
            auto_start_display: false,
        }
    }
}

struct DesktopState {
    settings: Mutex<AppSettings>,
    media_dir: PathBuf,
    data_dir: PathBuf,
    settings_path: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MonitorInfo {
    index: usize,
    name: String,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    is_primary: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusInfo {
    port: u16,
    media_dir: String,
    data_dir: String,
    server_url: String,
    admin_url: String,
    display_url: String,
    display_monitor_index: usize,
    auto_start_display: bool,
}

fn load_settings(path: &PathBuf) -> AppSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings_file(path: &PathBuf, settings: &AppSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_status(state: State<'_, Arc<DesktopState>>) -> StatusInfo {
    let settings = state.settings.lock().clone();
    StatusInfo {
        port: settings.port,
        media_dir: state.media_dir.display().to_string(),
        data_dir: state.data_dir.display().to_string(),
        server_url: format!("http://127.0.0.1:{}", settings.port),
        admin_url: format!("http://127.0.0.1:{}/admin", settings.port),
        display_url: format!("http://127.0.0.1:{}/", settings.port),
        display_monitor_index: settings.display_monitor_index,
        auto_start_display: settings.auto_start_display,
    }
}

#[tauri::command]
fn list_monitors(app: AppHandle) -> Result<Vec<MonitorInfo>, String> {
    let primary = app.primary_monitor().map_err(|e| e.to_string())?;
    let primary_pos = primary.as_ref().map(|m| m.position());
    let monitors = app.available_monitors().map_err(|e| e.to_string())?;

    Ok(monitors
        .into_iter()
        .enumerate()
        .map(|(index, monitor)| {
            let pos = monitor.position();
            let size = monitor.size();
            let is_primary = primary_pos
                .map(|p| p.x == pos.x && p.y == pos.y)
                .unwrap_or(false);
            MonitorInfo {
                index,
                name: monitor
                    .name()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("Display {}", index + 1)),
                width: size.width,
                height: size.height,
                x: pos.x,
                y: pos.y,
                is_primary,
            }
        })
        .collect())
}

#[tauri::command]
fn save_settings(
    state: State<'_, Arc<DesktopState>>,
    port: u16,
    display_monitor_index: usize,
    auto_start_display: bool,
) -> Result<StatusInfo, String> {
    if !(1..=65535).contains(&port) {
        return Err("Port must be between 1 and 65535".into());
    }
    let mut settings = state.settings.lock();
    settings.port = port;
    settings.display_monitor_index = display_monitor_index;
    settings.auto_start_display = auto_start_display;
    save_settings_file(&state.settings_path, &settings)?;
    drop(settings);
    Ok(get_status(state))
}

#[tauri::command]
fn open_media_folder(app: AppHandle, state: State<'_, Arc<DesktopState>>) -> Result<(), String> {
    std::fs::create_dir_all(&state.media_dir).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(state.media_dir.display().to_string(), None::<String>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_admin(app: AppHandle, state: State<'_, Arc<DesktopState>>) -> Result<(), String> {
    let port = state.settings.lock().port;
    app.opener()
        .open_url(
            format!("http://127.0.0.1:{port}/admin?desktop=1"),
            None::<String>,
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_docs(app: AppHandle, state: State<'_, Arc<DesktopState>>) -> Result<(), String> {
    let port = state.settings.lock().port;
    app.opener()
        .open_url(format!("http://127.0.0.1:{port}/openapi.json"), None::<String>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn show_display(app: AppHandle) -> Result<(), String> {
    let desktop = app.state::<Arc<DesktopState>>();
    let settings = desktop.settings.lock().clone();
    let monitors = list_monitors(app.clone())?;
    let monitor = monitors
        .get(settings.display_monitor_index)
        .or_else(|| monitors.first())
        .ok_or_else(|| "No monitors available".to_string())?;

    let url = format!("http://127.0.0.1:{}/", settings.port);

    if let Some(existing) = app.get_webview_window("display") {
        existing.close().map_err(|e| e.to_string())?;
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }

    let window = WebviewWindowBuilder::new(
        &app,
        "display",
        WebviewUrl::External(url.parse().map_err(|e: url::ParseError| e.to_string())?),
    )
    .title("LoopServe Display")
    .visible(false)
    .decorations(false)
    .always_on_top(false)
    .build()
    .map_err(|e| e.to_string())?;

    window
        .set_position(PhysicalPosition::new(monitor.x, monitor.y))
        .map_err(|e| e.to_string())?;
    window
        .set_size(PhysicalSize::new(monitor.width, monitor.height))
        .map_err(|e| e.to_string())?;
    window.set_fullscreen(true).map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn hide_display(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("display") {
        window.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let app_data = app
                .path()
                .app_data_dir()
                .expect("app data dir")
                .join("loopserve");
            let media_dir = app_data.join("media");
            let data_dir = app_data.join("data");
            let settings_path = app_data.join("settings.json");
            std::fs::create_dir_all(&media_dir)?;
            std::fs::create_dir_all(&data_dir)?;

            // Dev: use repo resources; prod: bundled resources
            let public_dir = {
                let resource = app
                    .path()
                    .resource_dir()
                    .ok()
                    .map(|p| p.join("resources").join("public"));
                if let Some(path) = resource.filter(|p| p.join("index.html").exists()) {
                    path
                } else {
                    // fallback for `tauri dev`
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/public")
                }
            };

            let mut settings = load_settings(&settings_path);
            // Prefer a secondary monitor for output when available.
            if let Ok(monitors) = app.available_monitors() {
                if settings.display_monitor_index == 0 && monitors.len() > 1 {
                    if let Ok(Some(primary)) = app.primary_monitor() {
                        let primary_pos = primary.position();
                        if let Some((index, _)) = monitors.iter().enumerate().find(|(_, m)| {
                            let pos = m.position();
                            pos.x != primary_pos.x || pos.y != primary_pos.y
                        }) {
                            settings.display_monitor_index = index;
                            let _ = save_settings_file(&settings_path, &settings);
                        }
                    }
                }
            }
            let port = settings.port;

            let desktop = Arc::new(DesktopState {
                settings: Mutex::new(settings.clone()),
                media_dir: media_dir.clone(),
                data_dir: data_dir.clone(),
                settings_path,
            });
            app.manage(desktop.clone());

            let server_state = AppState::new(ServerPaths {
                media_dir,
                data_dir,
                public_dir,
            });

            tauri::async_runtime::spawn(async move {
                if let Err(err) = start_server(server_state, port).await {
                    eprintln!("LoopServe media server failed: {err}");
                }
            });

            if settings.auto_start_display {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // Wait until the embedded server is actually listening.
                    for _ in 0..40 {
                        if tokio::net::TcpStream::connect(("127.0.0.1", port))
                            .await
                            .is_ok()
                        {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    let _ = show_display(handle).await;
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_monitors,
            save_settings,
            open_media_folder,
            open_admin,
            open_docs,
            show_display,
            hide_display
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
