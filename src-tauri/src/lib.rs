mod commands;
#[cfg(target_os = "linux")]
mod desktop_dnd;
mod downloads;
mod links;
pub mod profile;
#[cfg(target_os = "linux")]
mod secrets;
#[cfg(target_os = "linux")]
mod single_instance;
mod theme;
mod transcription;
mod tray;

use tauri::{
    webview::DownloadEvent, DragDropEvent, Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
    WindowEvent, Theme,
};

const INIT_SCRIPT: &str = include_str!("../../src/injection.js");
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

fn tauri_theme_from_dark(is_dark: bool) -> Theme {
    if is_dark {
        Theme::Dark
    } else {
        Theme::Light
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let prof = profile::get();
    let window_title = prof.window_title();
    let data_dir = prof.data_dir.clone();
    let config_dir = prof.config_dir.clone();
    let start_minimized = prof.start_minimized;

    std::fs::create_dir_all(&data_dir).ok();
    std::fs::create_dir_all(&config_dir).ok();

    // Restore Do Not Disturb before the tray builds its menu: build_menu snapshots
    // DND_ENABLED into the CheckMenuItem, so this must happen first.
    commands::DND_ENABLED.store(commands::load_dnd(), std::sync::atomic::Ordering::Relaxed);
    downloads::ASK_LOCATION.store(
        downloads::load_ask_location(),
        std::sync::atomic::Ordering::Relaxed,
    );
    #[cfg(target_os = "linux")]
    desktop_dnd::FOLLOW.store(
        desktop_dnd::load_follow(),
        std::sync::atomic::Ordering::Relaxed,
    );

    // Claim the profile before touching the WebKit data directory: a second
    // instance sharing it can corrupt the stored session.
    #[cfg(target_os = "linux")]
    let instance_guard = match single_instance::acquire(&data_dir, links::launch_url()) {
        single_instance::Instance::AlreadyRunning => {
            eprintln!(
                "walz is already running for profile '{}'; raising its window",
                prof.name
            );
            return;
        }
        single_instance::Instance::Primary(listener) => listener,
    };

    // Only once this process owns the profile: the staging directory belongs to
    // whichever instance is running, and a launch that hands off and exits must
    // not delete a download the incumbent is still waiting on.
    downloads::clean_stage_root();

    // A link on the command line has to wait: nothing listens for events until
    // the injected script runs, so it is parked for the page to collect. A
    // launch that handed off above never gets here -- the incumbent reports on
    // that link instead.
    if let Some(url) = links::launch_url() {
        links::park(url);
    }

    // Opt-in experiment: let WebKit deliver native HTML5 drops straight to
    // WhatsApp's own drop zone instead of routing them through Rust.
    let native_dnd = std::env::var_os("WALZ_NATIVE_DND").is_some();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(move |app| {
            let handle = app.handle().clone();

            #[cfg(target_os = "linux")]
            let initial_dark = tauri::async_runtime::block_on(theme::get_system_dark_mode()).ok();

            #[cfg(not(target_os = "linux"))]
            let initial_dark = None;

            let mut builder = WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::External("https://web.whatsapp.com".parse().unwrap()),
            )
            .title(&window_title)
            .inner_size(1000.0, 600.0)
            .min_inner_size(800.0, 600.0)
            .data_directory(data_dir.clone())
            .user_agent(USER_AGENT)
            .enable_clipboard_access()
            .decorations(true)
            .resizable(true)
            .visible(!start_minimized)
            .theme(initial_dark.map(tauri_theme_from_dark))
            .initialization_script(INIT_SCRIPT)
            .on_download(move |webview, event| {
                match event {
                    DownloadEvent::Requested { url, destination } => {
                        downloads::on_requested(&url, destination);
                    }
                    DownloadEvent::Finished { path, success, .. } => {
                        downloads::on_finished(webview.app_handle(), path, success);
                    }
                    _ => {}
                }
                true
            });

            if native_dnd {
                builder = builder.disable_drag_drop_handler();
            }

            let window = builder.build()?;

            tray::setup_tray(app)?;

            #[cfg(target_os = "linux")]
            if let Some(listener) = instance_guard {
                single_instance::serve(listener, handle.clone());
            }

            #[cfg(target_os = "linux")]
            desktop_dnd::watch(handle.clone());

            let window_clone = window.clone();
            let theme_event_handle = handle.clone();
            window.on_window_event(move |event| match event {
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = window_clone.hide();
                }
                WindowEvent::ThemeChanged(theme) => {
                    let _ = theme_event_handle.emit(
                        "system-theme-changed",
                        matches!(theme, Theme::Dark),
                    );
                }
                // Emit a serialized payload rather than interpolating into an
                // eval'd script: the old version injected the filename into a JS
                // string literal unescaped, so a file named `a".alert(1)//.png`
                // executed arbitrary JS in the WhatsApp origin.
                WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) => {
                    use base64::Engine;

                    let files: Vec<commands::ClipboardFile> = paths
                        .iter()
                        .filter_map(|path| {
                            let data = std::fs::read(path).ok()?;
                            Some(commands::ClipboardFile {
                                name: path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("file")
                                    .to_string(),
                                mime: mime_guess::from_path(path)
                                    .first_or_octet_stream()
                                    .to_string(),
                                data: base64::engine::general_purpose::STANDARD.encode(&data),
                            })
                        })
                        .collect();

                    if !files.is_empty() {
                        let _ = window_clone.emit("files-dropped", files);
                    }
                }
                _ => {}
            });

            #[cfg(target_os = "linux")]
            {
                let theme_window = window.clone();
                let theme_handle = handle.clone();
                std::thread::spawn(move || {
                    let mut last_theme = None;

                    loop {
                        if let Ok(is_dark) =
                            tauri::async_runtime::block_on(theme::get_system_dark_mode())
                        {
                            if last_theme != Some(is_dark) {
                                last_theme = Some(is_dark);
                                let _ = theme_window.set_theme(Some(tauri_theme_from_dark(is_dark)));
                                let _ = theme_handle.emit("system-theme-changed", is_dark);
                            }
                        }

                        std::thread::sleep(std::time::Duration::from_secs(2));
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::set_pending_download_name,
            commands::take_pending_link,
            commands::send_notification,
            commands::get_system_theme,
            commands::update_badge,
            commands::get_custom_css,
            commands::get_zoom,
            commands::save_zoom,
            commands::get_clipboard_files,
            transcription::transcribe_audio,
            transcription::translate_transcript,
            #[cfg(target_os = "linux")]
            commands::store_secret,
            #[cfg(target_os = "linux")]
            commands::get_secret,
            #[cfg(target_os = "linux")]
            commands::delete_secret,
            #[cfg(target_os = "linux")]
            commands::update_mpris_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
