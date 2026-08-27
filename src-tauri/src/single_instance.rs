//! Profile-aware single-instance guard.
//!
//! Two instances sharing one WebKit data directory can corrupt the stored
//! session, but `tauri-plugin-single-instance` keys its lock on the bundle
//! identifier with no override -- every profile would contend for the same lock
//! and `--profile work` could not run beside the default profile.
//!
//! So we bind an abstract Unix socket named after the profile's data directory.
//! The kernel releases an abstract name as soon as the owning process dies, so
//! there are no stale lock files to clean up, and the socket doubles as the
//! channel a second launch uses to raise the first instance's window.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::path::Path;

use tauri::AppHandle;

const WAKE: &[u8] = b"walz-show";
const MAX_MESSAGE: u64 = 8 * 1024;

pub enum Instance {
    /// This process owns the profile. Hold the listener for the app's lifetime;
    /// `None` means the guard could not be established and we run unguarded.
    Primary(Option<UnixListener>),
    /// Another instance owns the profile and has been asked to surface.
    AlreadyRunning,
}

/// Abstract socket names live in a per-network-namespace table shared by all
/// users, so the name must distinguish users as well as profiles. The data
/// directory encodes both, and hashing it keeps us inside the 107-byte limit.
fn socket_name(data_dir: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    data_dir.hash(&mut hasher);
    format!("walz-{:016x}", hasher.finish())
}

/// A later launch may carry a link to open, so the wake-up is a prefix rather
/// than the whole message: `WAKE` optionally followed by the URL.
pub fn acquire(data_dir: &Path, url: Option<&str>) -> Instance {
    let name = socket_name(data_dir);
    let Ok(addr) = SocketAddr::from_abstract_name(name.as_bytes()) else {
        // Cannot even name the socket; prefer starting over refusing to start.
        return Instance::Primary(None);
    };

    match UnixListener::bind_addr(&addr) {
        Ok(listener) => Instance::Primary(Some(listener)),
        Err(_) => match UnixStream::connect_addr(&addr) {
            // The incumbent answered: hand off and let this process exit.
            Ok(mut stream) => {
                let _ = stream.write_all(WAKE);
                if let Some(url) = url {
                    let _ = stream.write_all(url.as_bytes());
                }
                let _ = stream.flush();
                Instance::AlreadyRunning
            }
            // Name is taken but nobody is listening. Abstract names die with
            // their process, so this should not happen -- fail open rather than
            // leave the user unable to launch.
            Err(_) => Instance::Primary(None),
        },
    }
}

/// Serve wake-up requests from later launches for the rest of the app's life.
pub fn serve(listener: UnixListener, app: AppHandle) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };

            // Cap the read: the peer is another walz, but a stuck or hostile
            // writer should not be able to grow this buffer without bound.
            let mut message = Vec::new();
            if stream.take(MAX_MESSAGE).read_to_end(&mut message).is_err() {
                continue;
            }
            let Some(url) = message.strip_prefix(WAKE) else {
                continue;
            };

            if let Some(url) = std::str::from_utf8(url).ok().filter(|u| !u.is_empty()) {
                crate::links::handle(&app, url);
            }
            crate::tray::show_main_window(&app);
        }
    });
}
