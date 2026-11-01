//! The Shell: Ersa's window (ARCH-CORE, Shell). It owns the webview and the
//! connection to Core, and has no business logic.
//!
//! - [`launch`]: attach to the user's running Core, or start one.
//! - [`bridge`]: the connection itself, which the Interface reaches through
//!   two commands and two events:
//!   - `core_attach` attaches (launching Core if needed), completes the
//!     handshake, and returns `{ how: "attached" | "launched", hello }`, where
//!     `hello` is Core's `core.hello` result. Attaching again while connected
//!     returns the same handshake.
//!   - `core_request { method, params }` sends one Core API request and
//!     returns its result, or a [`bridge::BridgeError`].
//!   - `core-notification` carries each `{ method, params }` Core sends.
//!   - `core-connection` carries `{ state: "lost", reason }` when the
//!     connection drops; the Interface attaches again when it chooses.

pub mod bridge;
pub mod launch;

use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

use bridge::{BridgeError, CoreLink, LinkEvent};
use launch::CoreLocation;

/// What the Shell keeps between commands.
struct Shell {
    /// Where Core is, or why we could not work that out.
    location: Result<CoreLocation, String>,
    /// The open connection, if any. An async mutex because attaching holds it
    /// across the launch, so two attaches never launch two Cores.
    link: Mutex<Option<Arc<CoreLink>>>,
}

#[derive(Clone, Serialize)]
struct ConnectionLost {
    state: &'static str,
    reason: String,
}

#[tauri::command]
async fn core_attach(app: AppHandle, shell: State<'_, Shell>) -> Result<Value, BridgeError> {
    let mut link = shell.link.lock().await;
    if let Some(open) = link.as_ref().filter(|l| !l.is_closed()) {
        return Ok(json!({ "how": "attached", "hello": open.hello() }));
    }
    let location = shell.location.as_ref().map_err(BridgeError::unavailable)?;
    let (stream, how) = launch::attach_or_launch(location)
        .await
        .map_err(|e| BridgeError::unavailable(e.to_string()))?;
    let opened = CoreLink::open(stream, move |event| match event {
        LinkEvent::Notification { method, params } => {
            let _ = app.emit(
                "core-notification",
                json!({ "method": method, "params": params }),
            );
        }
        LinkEvent::Closed { reason } => {
            let lost = ConnectionLost {
                state: "lost",
                reason,
            };
            let _ = app.emit("core-connection", lost);
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

/// Opens the window. Called by `main`.
pub fn run() {
    let shell = Shell {
        location: CoreLocation::default_for_user().map_err(|e| format!("cannot locate Core: {e}")),
        link: Mutex::new(None),
    };
    tauri::Builder::default()
        .manage(shell)
        .invoke_handler(tauri::generate_handler![core_attach, core_request])
        .run(tauri::generate_context!())
        .expect("the Tauri application failed to start");
}
