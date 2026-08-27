//! Download handling.
//!
//! WebKit asks for a destination synchronously on the main thread, so a save
//! dialog cannot be shown there -- it needs the same main loop to run. Instead a
//! download is staged into the profile data dir, and once it finishes the user
//! is asked where to keep it and the file is moved there. With the "Ask where to
//! save" toggle off, files go straight to ~/Downloads/Walz as before.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use url::Url;

pub static ASK_LOCATION: AtomicBool = AtomicBool::new(true);

/// Names the per-download staging directory. A directory rather than a filename
/// suffix so the staged file keeps its real name, which the save dialog reuses.
static STAGE_SEQ: AtomicU64 = AtomicU64::new(0);

const STAGE_SUBDIR: &str = "incomplete";

fn stage_root() -> PathBuf {
    crate::profile::get().data_dir.join(STAGE_SUBDIR)
}

/// ~/Downloads/Walz -- the destination when the user is not asked, and the
/// directory the save dialog opens in.
fn default_dir() -> Option<PathBuf> {
    let dirs = directories::UserDirs::new()?;
    Some(dirs.download_dir()?.join("Walz"))
}

/// The name to save under: what the page asked for, else the name WebKit
/// guessed, else the last URL segment. "download" is WebKit's placeholder for
/// "no idea", so it is only used when nothing better turns up.
fn resolve_filename(url: &Url, destination: &Path) -> OsString {
    crate::commands::PENDING_DOWNLOAD_NAME
        .lock()
        .ok()
        .and_then(|mut g| g.take())
        .map(OsString::from)
        .or_else(|| {
            destination
                .file_name()
                .filter(|n| *n != "download")
                .map(|n| n.to_os_string())
        })
        .or_else(|| {
            url.path_segments()
                .and_then(|mut s| s.next_back())
                .filter(|s| !s.is_empty() && *s != "download")
                .map(OsString::from)
        })
        .unwrap_or_else(|| OsString::from("download"))
}

/// Pick the path WebKit writes to. Must not block: it runs inside WebKit's
/// destination decision on the main thread.
pub fn on_requested(url: &Url, destination: &mut PathBuf) {
    let filename = resolve_filename(url, destination);

    if ASK_LOCATION.load(Ordering::Relaxed) {
        let dir = stage_root().join(STAGE_SEQ.fetch_add(1, Ordering::Relaxed).to_string());
        if std::fs::create_dir_all(&dir).is_ok() {
            *destination = dir.join(filename);
            return;
        }
        // Staging failed (read-only home?); fall through and save directly
        // rather than losing the download.
    }

    if let Some(dir) = default_dir() {
        std::fs::create_dir_all(&dir).ok();
        *destination = dir.join(filename);
    }
}

pub fn on_finished(app: &AppHandle, path: Option<PathBuf>, success: bool) {
    let Some(path) = path else {
        if success {
            notify(app, "File downloaded successfully");
        }
        return;
    };

    if !success {
        discard(&path);
        return;
    }

    if path.starts_with(stage_root()) {
        prompt_for_location(app, path);
    } else {
        notify(app, &format!("Saved to {}", pretty(&path)));
    }
}

/// Ask where to keep a staged file, then move it there. The dialog is async: its
/// callback fires later, off the main loop, so the move happens on its own
/// thread in case the target is on another disk and needs a real copy.
fn prompt_for_location(app: &AppHandle, staged: PathBuf) {
    let name = staged
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".to_string());

    let mut dialog = app.dialog().file().set_title("Save file").set_file_name(&name);
    if let Some(dir) = default_dir() {
        std::fs::create_dir_all(&dir).ok();
        dialog = dialog.set_directory(dir);
    }
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }

    let app = app.clone();
    dialog.save_file(move |chosen| match chosen.and_then(|p| p.into_path().ok()) {
        Some(target) => {
            std::thread::spawn(move || match relocate(&staged, &target) {
                Ok(()) => notify(&app, &format!("Saved to {}", pretty(&target))),
                Err(e) => {
                    eprintln!("walz: could not save download to {}: {e}", target.display());
                    notify(&app, &format!("Could not save {name}"));
                }
            });
        }
        None => discard(&staged),
    });
}

/// Rename when possible, copy when the target is on a different filesystem.
fn relocate(staged: &Path, target: &Path) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::rename(staged, target) {
        Ok(()) => {}
        Err(_) => {
            std::fs::copy(staged, target)?;
            std::fs::remove_file(staged).ok();
        }
    }
    clear_stage_dir(staged);
    Ok(())
}

/// Cancelled or failed: drop the staged file and its directory.
fn discard(staged: &Path) {
    if !staged.starts_with(stage_root()) {
        return;
    }
    std::fs::remove_file(staged).ok();
    clear_stage_dir(staged);
}

fn clear_stage_dir(staged: &Path) {
    if let Some(dir) = staged.parent() {
        if dir.starts_with(stage_root()) && dir != stage_root() {
            std::fs::remove_dir(dir).ok();
        }
    }
}

/// Abbreviate the home directory so the notification stays readable.
fn pretty(path: &Path) -> String {
    let text = path.display().to_string();
    match dirs::home_dir().map(|h| h.display().to_string()) {
        Some(home) if text.starts_with(&home) => format!("~{}", &text[home.len()..]),
        _ => text,
    }
}

/// Download notifications bypass `commands::send_notification`, so they have to
/// honour Do Not Disturb themselves.
fn notify(app: &AppHandle, body: &str) {
    if crate::commands::DND_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let icon_path = crate::commands::notification_icon_path();
    let _ = app
        .notification()
        .builder()
        .title("Download Complete")
        .body(body)
        .icon(icon_path.to_string_lossy())
        .show();
}

fn ask_location_path() -> PathBuf {
    crate::profile::get().config_dir.join("ask-download-location")
}

/// Read the persisted toggle. Call before the tray menu is built: `build_menu`
/// snapshots `ASK_LOCATION` into its CheckMenuItem.
pub fn load_ask_location() -> bool {
    std::fs::read_to_string(ask_location_path())
        .map(|value| value.trim() != "false")
        .unwrap_or(true)
}

pub fn toggle_ask_location(app: &AppHandle) {
    let enabled = !ASK_LOCATION.load(Ordering::Relaxed);
    ASK_LOCATION.store(enabled, Ordering::Relaxed);
    let config = &crate::profile::get().config_dir;
    std::fs::create_dir_all(config).ok();
    std::fs::write(ask_location_path(), enabled.to_string()).ok();
    let _ = app.emit("set-ask-download-location", enabled);
    crate::tray::rebuild_menu(app);
}

/// Leftovers from a crash mid-download; harmless but they accumulate.
pub fn clean_stage_root() {
    std::fs::remove_dir_all(stage_root()).ok();
}
