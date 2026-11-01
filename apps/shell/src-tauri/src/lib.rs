//! The Shell: Ersa's window (ARCH-CORE, Shell). It owns the webview and the
//! connection to Core, and has no business logic.
//!
//! - [`launch`]: attach to the user's running Core, or start one.
//! - [`bridge`]: the connection itself, which the Interface reaches through
//!   two commands:
//!   - `core_attach { events }` attaches (launching Core if needed),
//!     completes the handshake, and returns `{ how: "attached" | "launched",
//!     hello }`, where `hello` is Core's `core.hello` result. Attaching again
//!     while connected returns the same handshake. `events` is a channel the
//!     Interface creates before calling; from then on it receives, in order,
//!     `{ kind: "notification", method, params }` for each notification Core
//!     sends and `{ kind: "lost", reason }` when the connection drops. A
//!     later attach's channel replaces it.
//!   - `core_request { method, params }` sends one Core API request and
//!     returns its result, or a [`bridge::BridgeError`].

pub mod bridge;
pub mod launch;

use std::sync::Arc;

use serde_json::{Value, json};
use tauri::ipc::Channel;
use tauri::webview::Url;
use tauri::{AppHandle, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;

use bridge::{BridgeError, CoreLink, LinkEvent, LinkOptions};
use launch::CoreLocation;

/// Where the Interface's events go: the channel of the latest `core_attach`.
type Sink = Arc<std::sync::Mutex<Option<Channel<Value>>>>;

/// What the Shell keeps between commands.
struct Shell {
    /// Where Core is, or why we could not work that out.
    location: Result<CoreLocation, String>,
    /// The open connection, if any. An async mutex because attaching holds it
    /// across the launch, so two attaches never launch two Cores.
    link: Mutex<Option<Arc<CoreLink>>>,
    sink: Sink,
}

#[tauri::command]
async fn core_attach(
    shell: State<'_, Shell>,
    events: Channel<Value>,
) -> Result<Value, BridgeError> {
    // Replace the sink before anything else, so nothing Core sends from here
    // on is lost or delivered to an Interface that no longer listens.
    *shell.sink.lock().expect("sink lock") = Some(events);
    let mut link = shell.link.lock().await;
    if let Some(open) = link.as_ref().filter(|l| !l.is_closed()) {
        return Ok(json!({ "how": "attached", "hello": open.hello() }));
    }
    let location = shell.location.as_ref().map_err(BridgeError::unavailable)?;
    let (stream, how) = launch::attach_or_launch(location)
        .await
        .map_err(|e| BridgeError::unavailable(e.to_string()))?;
    let sink = Arc::clone(&shell.sink);
    let options = LinkOptions {
        probe_endpoint: Some(location.endpoint.clone()),
        timeout: None,
    };
    let opened = CoreLink::open(stream, options, move |event| {
        let message = match event {
            LinkEvent::Notification { method, params } => {
                json!({ "kind": "notification", "method": method, "params": params })
            }
            LinkEvent::Closed { reason } => json!({ "kind": "lost", "reason": reason }),
        };
        if let Some(channel) = sink.lock().expect("sink lock").as_ref() {
            // Fails only when the webview is gone; nobody is left to tell.
            let _ = channel.send(message);
        }
    })
    .await?;
    let reply = json!({ "how": how.as_str(), "hello": opened.hello() });
    *link = Some(Arc::new(opened));
    Ok(reply)
}

#[tauri::command]
async fn core_request(
    shell: State<'_, Shell>,
    method: String,
    params: Option<Value>,
) -> Result<Value, BridgeError> {
    // Clone the link out so the lock is not held while Core answers.
    let link = shell.link.lock().await.clone();
    let link = link.ok_or_else(|| BridgeError::unavailable("not attached to Core"))?;
    link.request(&method, params).await
}

/// Whether the window may show `url`: only the app's own pages. The bundled
/// app is served from `tauri://localhost` (`http://tauri.localhost` on
/// Windows); a debug build also loads Vite's dev server on localhost.
fn is_app_origin(url: &Url) -> bool {
    let bundled = matches!(
        (url.scheme(), url.host_str()),
        ("tauri", Some("localhost")) | ("http" | "https", Some("tauri.localhost"))
    );
    let dev_server = url.scheme() == "http" && url.host_str() == Some("localhost");
    bundled || (cfg!(debug_assertions) && dev_server)
}

/// Keeps the window on the app: any other http(s) page opens in the user's
/// browser instead, and anything else is refused.
fn guard_navigation(app: &AppHandle, url: &Url) -> bool {
    if is_app_origin(url) {
        return true;
    }
    if matches!(url.scheme(), "http" | "https") {
        let _ = app.opener().open_url(url.as_str(), None::<&str>);
    }
    false
}

/// Opens the window. Called by `main`.
pub fn run() {
    let shell = Shell {
        location: CoreLocation::default_for_user().map_err(|e| format!("cannot locate Core: {e}")),
        link: Mutex::new(None),
        sink: Arc::new(std::sync::Mutex::new(None)),
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(shell)
        .invoke_handler(tauri::generate_handler![core_attach, core_request])
        // The window is built here rather than in tauri.conf.json so it can
        // carry the navigation guard.
        .setup(|app| {
            let handle = app.handle().clone();
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Ersa")
                .inner_size(1200.0, 800.0)
                .min_inner_size(640.0, 400.0)
                .on_navigation(move |url| guard_navigation(&handle, url))
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the Tauri application failed to start");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_stays_on_the_app() {
        let url = |s: &str| Url::parse(s).expect("a url");
        assert!(is_app_origin(&url("tauri://localhost/index.html")));
        assert!(is_app_origin(&url("http://tauri.localhost/index.html")));
        assert!(!is_app_origin(&url("https://example.com/")));
        assert!(!is_app_origin(&url("file:///C:/Windows/")));
        assert!(!is_app_origin(&url("http://localhost.example.com/")));
    }
}
