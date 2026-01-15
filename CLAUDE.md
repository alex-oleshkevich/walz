# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Walz is a WhatsApp desktop client for Linux built with Tauri 2.0. It wraps WhatsApp Web (https://web.whatsapp.com) in a native application with system integration features like tray icon, notifications, MPRIS media controls, and Secret Service storage.

## Build Commands

```bash
# Development
npm run dev                    # Start with hot reload
cargo build                    # Build Rust backend (from src-tauri/)

# Production
npm run build                  # Build release binary
cargo build --release          # Build release directly

# Arch Linux package
makepkg -si                    # Build and install from PKGBUILD
```

## Architecture

**Rust Backend** (`src-tauri/src/`):
- `lib.rs` - App setup: WebviewBuilder with external URL, plugins, download handler, drag-drop support, MPRIS server spawn
- `commands.rs` - Tauri IPC commands exposed to frontend (notifications, theme, badge, zoom, DND, privacy blur, secrets)
- `profile.rs` - Multi-profile support via `--profile <name>` CLI flag, manages separate data/config directories per profile
- `tray.rs` - System tray with context menu (show/hide, DND, privacy blur, zoom, quit), badge tooltip updates
- `theme.rs` - D-Bus XDG Portal query for system dark mode (`org.freedesktop.portal.Settings`)
- `mpris.rs` - MPRIS D-Bus server for media controls (play/pause/seek voice messages)
- `secrets.rs` - Secret Service API for secure credential storage in system keyring

**JavaScript Frontend** (`src/injection.js`):
- Injected into WhatsApp Web via `initialization_script()`
- Intercepts `Notification` constructor → routes to native notifications
- Monitors `document.title` for unread count → updates tray badge
- Applies system theme, handles privacy blur, loads custom CSS from `~/.config/walz/custom.css`
- Keyboard shortcuts: Ctrl+F (search), Ctrl+±0 (zoom), Ctrl+N (new chat), Esc (close)
- MPRIS event listeners for controlling audio elements

## Key Patterns

**Event Flow**: Rust → JS uses `app.emit("event-name", payload)`, JS listens via `window.__TAURI__.event.listen()`

**Global State**: Atomic flags in `commands.rs` (`DND_ENABLED`, `PRIVACY_BLUR_ENABLED`, `CURRENT_BADGE`)

**Profile-aware Paths**: Always use `crate::profile::get().config_dir` / `data_dir` instead of hardcoded paths

**Linux-only Code**: Gate with `#[cfg(target_os = "linux")]` for MPRIS, secrets, and D-Bus features

## Configuration

- **Data**: `~/.local/share/walz/` (WebKit data, session)
- **Config**: `~/.config/walz/` (custom.css, zoom)
- **Profiles**: `~/.local/share/walz/profiles/<name>/` and `~/.config/walz/profiles/<name>/`

## User Agent

Custom Chrome UA required for WhatsApp Web compatibility - defined in `lib.rs` as `USER_AGENT` constant.
