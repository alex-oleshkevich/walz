use std::sync::atomic::Ordering;
use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Emitter, Manager,
};

use crate::commands::DND_ENABLED;
use crate::profile;

const TRAY_ID: &str = "main-tray";
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray-icon.png");

fn create_badge_icon(count: u32) -> Option<Image<'static>> {
    use image::{ImageBuffer, Rgba, RgbaImage};

    let base_img = image::load_from_memory(TRAY_ICON).ok()?.to_rgba8();
    let (width, height) = base_img.dimensions();
    let mut img: RgbaImage = ImageBuffer::from_raw(width, height, base_img.into_raw())?;

    if count == 0 {
        let rgba = img.into_raw();
        return Some(Image::new_owned(rgba, width, height));
    }

    let badge_radius = (width.min(height) / 6) as i32;
    let center_x = (width as i32) - badge_radius - 2;
    let center_y = badge_radius + 2;

    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let dx = x - center_x;
            let dy = y - center_y;
            let dist_sq = dx * dx + dy * dy;
            let radius_sq = badge_radius * badge_radius;
            if dist_sq <= radius_sq {
                img.put_pixel(x as u32, y as u32, Rgba([239, 68, 68, 255]));
            }
        }
    }

    let rgba = img.into_raw();
    Some(Image::new_owned(rgba, width, height))
}

pub fn setup_tray(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let menu = build_menu(app)?;
    let tooltip = profile::get().tray_tooltip();

    let icon = Image::from_bytes(TRAY_ICON)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(&tooltip)
        .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()))
        .on_tray_icon_event(|tray: &tauri::tray::TrayIcon, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

fn build_menu<M: Manager<tauri::Wry>>(app: &M) -> Result<Menu<tauri::Wry>, tauri::Error> {
    let show = MenuItem::with_id(app, "show", "Show Walz", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;

    let dnd = CheckMenuItem::with_id(
        app,
        "dnd",
        "Do Not Disturb",
        true,
        DND_ENABLED.load(Ordering::Relaxed),
        None::<&str>,
    )?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    // Built with the `devtools` cargo feature so this works in release builds
    // too -- a webview bug reported by a packaged user is otherwise unreachable.
    let devtools = MenuItem::with_id(app, "devtools", "Open DevTools", true, None::<&str>)?;
    let sep3 = PredefinedMenuItem::separator(app)?;

    let quit = MenuItem::with_id(app, "quit", "Quit", true, Some("Ctrl+Q"))?;

    Menu::with_items(
        app,
        &[
            &show,
            &hide,
            &sep1,
            &dnd,
            &sep2,
            &devtools,
            &sep3,
            &quit,
        ],
    )
}

/// Raise the main window. Used by the tray and by a second launch handing off
/// to this instance (see `single_instance`).
pub fn show_main_window(app: &AppHandle) {
    show_window_with_chat(app);
}

fn show_window_with_chat(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    if let Some(chat_id) = crate::commands::take_pending_chat() {
        let _ = app.emit("notification-clicked", chat_id);
    }
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "show" => {
            show_window_with_chat(app);
        }
        "hide" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.hide();
            }
        }
        "dnd" => {
            crate::commands::toggle_dnd(app);
        }
        "devtools" => {
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }
        }
        "quit" => {
            app.exit(0);
        }
        _ => {}
    }
}

fn toggle_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            show_window_with_chat(app);
        }
    }
}

pub fn update_tray_badge(app: &AppHandle, count: u32) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let base = profile::get().tray_tooltip();
        let tooltip = if count > 0 {
            format!("{} ({} unread)", base, count)
        } else {
            base
        };
        let _ = tray.set_tooltip(Some(&tooltip));

        if let Some(icon) = create_badge_icon(count) {
            let _ = tray.set_icon(Some(icon));
        }
    }
}

pub fn rebuild_menu(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(menu) = build_menu(app) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}
