//! Click-to-chat link handling for `whatsapp:` URLs and their https twins.
//!
//! WhatsApp Web opens a conversation from a `/send?phone=` URL, so every link
//! shape we accept is translated into one of those and handed to the webview.
//! Anything we cannot translate returns None and the caller just raises the
//! window -- better than navigating away from the chat list to an error page.

use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Emitter};
use url::{form_urlencoded, Url};

/// The URL this process was launched with, or the last one a second launch
/// handed over. Taken by the injected script once it is ready to navigate.
static PENDING: Mutex<Option<String>> = Mutex::new(None);
static LAUNCH_URL: OnceLock<Option<String>> = OnceLock::new();

/// Record the URL from the command line. Called during profile init, before the
/// single-instance guard decides whether this process is the one that runs.
pub fn set_launch_url(url: Option<String>) {
    let _ = LAUNCH_URL.set(url);
}

pub fn launch_url() -> Option<&'static str> {
    LAUNCH_URL.get()?.as_deref()
}

pub fn take_pending() -> Option<String> {
    PENDING.lock().ok()?.take()
}

/// Park the link this process was launched with. Nothing listens for events
/// until the injected script runs, so it waits to be collected.
pub fn park(raw: &str) {
    if let Some(target) = translate(raw) {
        if let Ok(mut pending) = PENDING.lock() {
            *pending = Some(target);
        }
    }
}

/// Route a link that arrived after startup, from a launch that handed off to
/// this instance. The page is already loaded, so it can navigate straight away.
pub fn handle(app: &AppHandle, raw: &str) {
    if let Some(target) = translate(raw) {
        let _ = app.emit("open-url", target);
    }
}

fn translate(raw: &str) -> Option<String> {
    let target = to_web_url(raw);
    if target.is_none() {
        eprintln!("walz: don't know how to open {raw}");
    }
    target
}

/// Translate a click-to-chat link into the web.whatsapp.com URL that opens it.
///
/// Handles `whatsapp://send?phone=`, `wa.me/<number>`, `api.whatsapp.com/send`,
/// and group invites from `chat.whatsapp.com`. Short links (`wa.me/message/...`)
/// resolve server-side and cannot be mapped without following them, so they are
/// rejected rather than guessed at.
pub fn to_web_url(raw: &str) -> Option<String> {
    let url = Url::parse(raw.trim()).ok()?;

    // `whatsapp://send?...` puts "send" in the host, `whatsapp:send?...` puts it
    // in the path. Accept either spelling.
    let action = url
        .host_str()
        .filter(|host| !host.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| url.path().trim_start_matches('/').to_string());

    let param = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };

    match url.scheme() {
        "whatsapp" => match action.as_str() {
            "send" => send_url(param("phone").as_deref(), param("text").as_deref()),
            "chat" => param("code").as_deref().map(invite_url),
            _ => None,
        },
        "http" | "https" => match url.host_str()? {
            "wa.me" => {
                let path = url.path().trim_matches('/');
                // wa.me/message/<id> and wa.me/qr/<id> are server-side redirects.
                if path.is_empty() || path.contains('/') {
                    return None;
                }
                send_url(Some(path), param("text").as_deref())
            }
            "api.whatsapp.com" | "web.whatsapp.com" => {
                send_url(param("phone").as_deref(), param("text").as_deref())
            }
            "chat.whatsapp.com" => {
                let code = url.path().trim_matches('/');
                (!code.is_empty() && !code.contains('/')).then(|| invite_url(code))
            }
            _ => None,
        },
        _ => None,
    }
}

/// A phone number is required: `/send` without one lands on a "choose a contact"
/// screen, which is not what clicking a link should do.
fn send_url(phone: Option<&str>, text: Option<&str>) -> Option<String> {
    let digits: String = phone?.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }

    let mut params = vec![("phone", digits.as_str())];
    if let Some(text) = text.filter(|text| !text.is_empty()) {
        params.push(("text", text));
    }
    Some(web_url("send", &params))
}

fn invite_url(code: &str) -> String {
    web_url("accept", &[("code", code)])
}

fn web_url(path: &str, params: &[(&str, &str)]) -> String {
    let query = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish();
    format!("https://web.whatsapp.com/{path}?{query}")
}

#[cfg(test)]
mod tests {
    use super::to_web_url;

    #[test]
    fn maps_click_to_chat_links() {
        let expected = Some("https://web.whatsapp.com/send?phone=15551234567".to_string());
        assert_eq!(to_web_url("whatsapp://send?phone=15551234567"), expected);
        assert_eq!(to_web_url("https://wa.me/15551234567"), expected);
        assert_eq!(
            to_web_url("https://api.whatsapp.com/send?phone=%2B1%20(555)%20123-4567"),
            expected
        );
    }

    #[test]
    fn encodes_prefilled_text() {
        assert_eq!(
            to_web_url("https://wa.me/1555?text=hi%20there%20%26%20now"),
            Some("https://web.whatsapp.com/send?phone=1555&text=hi+there+%26+now".to_string())
        );
    }

    #[test]
    fn maps_group_invites() {
        assert_eq!(
            to_web_url("https://chat.whatsapp.com/ABC123"),
            Some("https://web.whatsapp.com/accept?code=ABC123".to_string())
        );
    }

    #[test]
    fn rejects_what_it_cannot_translate() {
        // Server-side redirects, a missing number, and unrelated links.
        assert_eq!(to_web_url("https://wa.me/message/ABC123"), None);
        assert_eq!(to_web_url("whatsapp://send?text=hello"), None);
        assert_eq!(to_web_url("https://example.com/wa.me/1555"), None);
        assert_eq!(to_web_url("not a url"), None);
    }
}
