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
- `commands.rs` - Tauri IPC commands exposed to frontend (notifications, theme, badge, zoom, DND, secrets)
- `downloads.rs` - Download staging, save dialog, and the "ask where to save" toggle
- `links.rs` - Translates `whatsapp:`/`wa.me` click-to-chat links into web.whatsapp.com URLs
- `desktop_dnd.rs` - Mirrors the desktop's own Do Not Disturb switch (Plasma `Inhibited`, GNOME `show-banners`)
- `profile.rs` - Multi-profile support via `--profile <name>` CLI flag, manages separate data/config directories per profile
- `tray.rs` - System tray with context menu (show/hide, DND, zoom, quit), badge tooltip updates
- `theme.rs` - D-Bus XDG Portal query for system dark mode (`org.freedesktop.portal.Settings`)
- `mpris.rs` - MPRIS D-Bus server for media controls (play/pause/seek voice messages)
- `secrets.rs` - Secret Service API for secure credential storage in system keyring

**JavaScript Frontend** (`src/injection.js`):
- Injected into WhatsApp Web via `initialization_script()`
- Intercepts `Notification` constructor → routes to native notifications
- Monitors `document.title` for unread count → updates tray badge
- Applies system theme and loads custom CSS from `~/.config/walz/custom.css`
- Keyboard shortcuts: Ctrl+F (search), Ctrl+±0 (zoom), Ctrl+N (new chat), Esc (close)
- MPRIS event listeners for controlling audio elements

## Key Patterns

**Event Flow**: Rust → JS uses `app.emit("event-name", payload)`, JS listens via `window.__TAURI__.event.listen()`

**Global State**: Atomic flags in `commands.rs` (`DND_ENABLED`, `CURRENT_BADGE`)

**Profile-aware Paths**: Always use `crate::profile::get().config_dir` / `data_dir` instead of hardcoded paths

**Linux-only Code**: Gate with `#[cfg(target_os = "linux")]` for MPRIS, secrets, and D-Bus features

**URL Handling**: `walz <url>` opens a click-to-chat link; a second launch forwards it to the running instance over the single-instance socket, and `walz.desktop` claims `x-scheme-handler/whatsapp`

## Configuration

- **Data**: `~/.local/share/walz/` (WebKit data, session)
- **Config**: `~/.config/walz/` (custom.css, zoom, dnd, ask-download-location, follow-desktop-dnd)
- **Profiles**: `~/.local/share/walz/profiles/<name>/` and `~/.config/walz/profiles/<name>/`

## User Agent

Custom Chrome UA required for WhatsApp Web compatibility - defined in `lib.rs` as `USER_AGENT` constant.


<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:6cd5cc61 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->
