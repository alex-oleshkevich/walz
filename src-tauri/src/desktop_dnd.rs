//! Mirror the desktop's own Do Not Disturb switch into walz.
//!
//! There is no portal for this, so each desktop is asked in its own dialect:
//! Plasma answers through the `Inhibited` property its notification server
//! publishes, GNOME through the `show-banners` GSetting. A desktop that speaks
//! neither reports None and the feature stays inert rather than guessing.
//!
//! Polled rather than subscribed, matching the system-theme watcher in `lib.rs`:
//! one property read every few seconds is cheaper than keeping two dialects of
//! change notification alive.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use tauri::AppHandle;

pub static FOLLOW: AtomicBool = AtomicBool::new(false);

const POLL_INTERVAL: Duration = Duration::from_secs(3);

fn follow_path() -> PathBuf {
    crate::profile::get().config_dir.join("follow-desktop-dnd")
}

/// Read the persisted toggle. Call before the tray menu is built: `build_menu`
/// snapshots `FOLLOW` into its CheckMenuItem.
pub fn load_follow() -> bool {
    std::fs::read_to_string(follow_path())
        .map(|value| value.trim() == "true")
        .unwrap_or(false)
}

pub fn toggle_follow(app: &AppHandle) {
    let enabled = !FOLLOW.load(Ordering::Relaxed);
    FOLLOW.store(enabled, Ordering::Relaxed);

    let config = &crate::profile::get().config_dir;
    std::fs::create_dir_all(config).ok();
    std::fs::write(follow_path(), enabled.to_string()).ok();

    crate::tray::rebuild_menu(app);

    // Adopt the desktop's state immediately rather than at the next poll, so the
    // menu click has a visible effect.
    if enabled {
        if let Some(state) = desktop_state() {
            crate::commands::set_dnd(app, state);
        }
    }
}

/// Follow the desktop until the app exits. Only acts while the toggle is on, and
/// only on a change, so a manual DND flip is not fought over on every tick.
pub fn watch(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last = None;

        loop {
            if FOLLOW.load(Ordering::Relaxed) {
                let state = desktop_state();
                if state.is_some() && state != last {
                    last = state;
                    if let Some(enabled) = state {
                        crate::commands::set_dnd(&app, enabled);
                    }
                }
            } else {
                // Forget the last reading, so re-enabling the toggle applies the
                // desktop's current state instead of waiting for it to change.
                last = None;
            }

            std::thread::sleep(POLL_INTERVAL);
        }
    });
}

/// True when the desktop is silencing notifications, None when it has no way to
/// say.
fn desktop_state() -> Option<bool> {
    plasma_inhibited().or_else(gnome_dnd)
}

/// Whether this desktop can be asked at all. Cached: the tray menu is rebuilt on
/// every toggle and this would otherwise be a D-Bus round trip each time.
pub fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| desktop_state().is_some())
}

/// Plasma publishes `Inhibited` on the notification server itself. Anything else
/// implementing org.freedesktop.Notifications without that property -- most
/// servers -- returns an InvalidArgs error, which reads as "cannot say".
#[cfg(target_os = "linux")]
fn plasma_inhibited() -> Option<bool> {
    use zbus::{proxy, Connection};

    #[proxy(
        interface = "org.freedesktop.Notifications",
        default_service = "org.freedesktop.Notifications",
        default_path = "/org/freedesktop/Notifications"
    )]
    trait Notifications {
        #[zbus(property)]
        fn inhibited(&self) -> zbus::Result<bool>;
    }

    tauri::async_runtime::block_on(async {
        let connection = Connection::session().await.ok()?;
        let proxy = NotificationsProxy::new(&connection).await.ok()?;
        proxy.inhibited().await.ok()
    })
}

/// GNOME's Do Not Disturb is the inverse of `show-banners`.
///
/// The schema ships with the GNOME libraries rather than with the session, so it
/// is readable on desktops that ignore it entirely -- following it there would
/// mean overriding the user's own DND from a setting nothing honours. Hence the
/// session check as well as the schema lookup, which is also what keeps
/// `Settings::new` from aborting the process on a missing schema.
#[cfg(target_os = "linux")]
fn gnome_dnd() -> Option<bool> {
    use gio::prelude::SettingsExt;
    use gio::{Settings, SettingsSchemaSource};

    const SCHEMA: &str = "org.gnome.desktop.notifications";

    let session = std::env::var("XDG_CURRENT_DESKTOP").ok()?;
    if !session.to_ascii_uppercase().split(':').any(|d| d == "GNOME") {
        return None;
    }

    SettingsSchemaSource::default()?.lookup(SCHEMA, true)?;
    Some(!Settings::new(SCHEMA).boolean("show-banners"))
}

#[cfg(not(target_os = "linux"))]
fn plasma_inhibited() -> Option<bool> {
    None
}

#[cfg(not(target_os = "linux"))]
fn gnome_dnd() -> Option<bool> {
    None
}
