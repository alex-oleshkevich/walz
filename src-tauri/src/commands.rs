use serde::Serialize;
use std::fs;
#[cfg(target_os = "linux")]
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Theme, Window};

#[cfg(not(target_os = "linux"))]
use tauri_plugin_notification::NotificationExt;

pub static DND_ENABLED: AtomicBool = AtomicBool::new(false);
pub static CURRENT_BADGE: AtomicU32 = AtomicU32::new(0);
pub static PENDING_DOWNLOAD_NAME: Mutex<Option<String>> = Mutex::new(None);

#[derive(Serialize, Clone)]
pub struct ClipboardFile {
    pub name: String,
    pub mime: String,
    pub data: String,
}

#[tauri::command]
pub fn set_pending_download_name(name: String) {
    *PENDING_DOWNLOAD_NAME.lock().unwrap() = Some(name);
}

const ICON_BYTES: &[u8] = include_bytes!("../icons/128x128.png");

fn config_dir() -> PathBuf {
    crate::profile::get().config_dir.clone()
}

fn notification_icon_path() -> PathBuf {
    let path = crate::profile::get().data_dir.join("notification-icon.png");
    if !path.exists() {
        let _ = fs::write(&path, ICON_BYTES);
    }
    path
}

#[tauri::command]
pub async fn send_notification(
    app: AppHandle,
    title: String,
    body: String,
    chat_id: Option<String>,
) -> Result<(), String> {
    if DND_ENABLED.load(Ordering::Relaxed) {
        return Ok(());
    }
    let icon_path = notification_icon_path();

    if let Some(ref id) = chat_id {
        set_pending_chat(Some(id.clone()));
    }

    #[cfg(target_os = "linux")]
    {
        let chat_id_clone = chat_id.clone();
        let app_clone = app.clone();

        std::thread::spawn(move || {
            let result = notify_rust::Notification::new()
                .summary(&title)
                .body(&body)
                .icon(&icon_path.to_string_lossy())
                .action("default", "Open")
                .show();

            if let Ok(handle) = result {
                handle.wait_for_action(|action| {
                    if action == "default" || action == "__closed" {
                        if action == "default" {
                            if let Some(window) = app_clone.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                            if let Some(id) = chat_id_clone.as_ref() {
                                let _ = app_clone.emit("notification-clicked", id.clone());
                            }
                        }
                    }
                });
            }
        });
    }

    #[cfg(not(target_os = "linux"))]
    {
        let builder = app
            .notification()
            .builder()
            .title(&title)
            .body(&body)
            .icon(icon_path.to_string_lossy());
        builder.show().map_err(|e| e.to_string())?;
    }

    Ok(())
}

static PENDING_CHAT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn set_pending_chat(chat_id: Option<String>) {
    if let Ok(mut pending) = PENDING_CHAT.lock() {
        *pending = chat_id;
    }
}

pub fn take_pending_chat() -> Option<String> {
    if let Ok(mut pending) = PENDING_CHAT.lock() {
        pending.take()
    } else {
        None
    }
}

#[tauri::command]
pub async fn get_system_theme(window: Window) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    let is_dark = match crate::theme::get_system_dark_mode().await {
        Ok(is_dark) => is_dark,
        Err(_) => matches!(window.theme(), Ok(Theme::Dark)),
    };

    #[cfg(not(target_os = "linux"))]
    let is_dark = matches!(window.theme(), Ok(Theme::Dark));

    Ok(if is_dark { "dark" } else { "light" }.to_string())
}

#[tauri::command]
pub async fn update_badge(app: AppHandle, count: u32) -> Result<(), String> {
    let old = CURRENT_BADGE.swap(count, Ordering::Relaxed);
    if old != count {
        crate::tray::update_tray_badge(&app, count);
    }
    Ok(())
}

#[tauri::command]
pub async fn get_custom_css(_app: AppHandle) -> Result<String, String> {
    let css_path = config_dir().join("custom.css");
    fs::read_to_string(&css_path).or(Ok(String::new()))
}

#[tauri::command]
pub async fn get_zoom(_app: AppHandle) -> Result<f64, String> {
    let zoom_path = config_dir().join("zoom");
    fs::read_to_string(&zoom_path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .map(Ok)
        .unwrap_or(Ok(1.0))
}

#[tauri::command]
pub async fn save_zoom(_app: AppHandle, zoom: f64) -> Result<(), String> {
    let config = config_dir();
    fs::create_dir_all(&config).ok();
    fs::write(config.join("zoom"), zoom.to_string()).map_err(|e| e.to_string())
}

fn dnd_path() -> PathBuf {
    config_dir().join("dnd")
}

/// Read the persisted Do Not Disturb state. Call before the tray menu is built,
/// since `build_menu` snapshots `DND_ENABLED` into its CheckMenuItem.
pub fn load_dnd() -> bool {
    fs::read_to_string(dnd_path())
        .map(|value| value.trim() == "true")
        .unwrap_or(false)
}

/// The single way to change Do Not Disturb. Previously the tray and the IPC
/// command each did this by hand and had drifted apart: only the command
/// rebuilt the tray menu, so the CheckMenuItem could show a stale state.
pub fn set_dnd(app: &AppHandle, enabled: bool) {
    DND_ENABLED.store(enabled, Ordering::Relaxed);
    let _ = app.emit("set-dnd", enabled);
    crate::tray::rebuild_menu(app);

    let config = config_dir();
    fs::create_dir_all(&config).ok();
    let _ = fs::write(dnd_path(), if enabled { "true" } else { "false" });
}

pub fn toggle_dnd(app: &AppHandle) -> bool {
    let new_state = !DND_ENABLED.load(Ordering::Relaxed);
    set_dnd(app, new_state);
    new_state
}

#[tauri::command]
pub async fn get_clipboard_files() -> Result<Vec<ClipboardFile>, String> {
    #[cfg(target_os = "linux")]
    {
        use base64::Engine;
        use url::Url;
        use wl_clipboard_rs::paste::{self, ClipboardType, MimeType, Seat};

        let mime_types = paste::get_mime_types_ordered(ClipboardType::Regular, Seat::Unspecified)
            .map_err(|error| error.to_string())?;

        // Apps often offer several image flavours (GIMP advertises tiff/bmp ahead of
        // png). Take the most web-friendly one rather than whichever comes first.
        const IMAGE_PREFERENCE: [(&str, &str); 5] = [
            ("image/png", "png"),
            ("image/jpeg", "jpg"),
            ("image/webp", "webp"),
            ("image/gif", "gif"),
            ("image/bmp", "bmp"),
        ];

        let image_mime = IMAGE_PREFERENCE
            .iter()
            .find(|(mime, _)| mime_types.iter().any(|offered| offered == mime))
            .map(|(mime, extension)| ((*mime).to_string(), (*extension).to_string()))
            .or_else(|| {
                mime_types
                    .iter()
                    .find(|mime| mime.starts_with("image/"))
                    .map(|mime| {
                        let extension = mime.rsplit('/').next().unwrap_or("bin");
                        (mime.clone(), extension.to_string())
                    })
            });

        if let Some((mime, extension)) = image_mime {
            let (mut reader, _) = paste::get_contents(
                ClipboardType::Regular,
                Seat::Unspecified,
                MimeType::Specific(&mime),
            )
            .map_err(|error| error.to_string())?;
            let mut data = Vec::new();
            reader
                .read_to_end(&mut data)
                .map_err(|error| error.to_string())?;
            if !data.is_empty() {
                return Ok(vec![ClipboardFile {
                    name: format!("pasted-image.{extension}"),
                    mime,
                    data: base64::engine::general_purpose::STANDARD.encode(data),
                }]);
            }
        }

        let Some(uri_mime) = mime_types
            .iter()
            .find(|mime| mime.as_str() == "text/uri-list")
        else {
            return Ok(Vec::new());
        };

        let (mut reader, _) = paste::get_contents(
            ClipboardType::Regular,
            Seat::Unspecified,
            MimeType::Specific(uri_mime),
        )
        .map_err(|error| error.to_string())?;
        let mut uri_data = String::new();
        reader
            .read_to_string(&mut uri_data)
            .map_err(|error| error.to_string())?;

        let mut files = Vec::new();
        for line in uri_data.lines() {
            let uri = line.trim();
            if uri.is_empty() || uri.starts_with('#') {
                continue;
            }

            let Ok(url) = Url::parse(uri) else {
                continue;
            };
            if url.scheme() != "file" {
                continue;
            }

            let Ok(path) = url.to_file_path() else {
                continue;
            };
            let Ok(data) = fs::read(&path) else {
                continue;
            };
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("pasted-file")
                .to_string();
            let mime = mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string();
            files.push(ClipboardFile {
                name,
                mime,
                data: base64::engine::general_purpose::STANDARD.encode(data),
            });
        }

        return Ok(files);
    }

    #[cfg(not(target_os = "linux"))]
    Ok(Vec::new())
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn store_secret(key: String, value: String) -> Result<(), String> {
    let profile = crate::profile::get().name.clone();
    crate::secrets::store_secret(&key, &value, &profile)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn get_secret(key: String) -> Result<Option<String>, String> {
    let profile = crate::profile::get().name.clone();
    crate::secrets::get_secret(&key, &profile)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn delete_secret(key: String) -> Result<(), String> {
    let profile = crate::profile::get().name.clone();
    crate::secrets::delete_secret(&key, &profile)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn update_mpris_status(_status: String) -> Result<(), String> {
    Ok(())
}
