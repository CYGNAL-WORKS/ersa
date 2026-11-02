//! The Shell: Ersa's window (ARCH-CORE, Shell). It owns the webview and the
//! connection to Core, and has no business logic.
//!
//! - [`launch`]: attach to the user's running Core, or start one.
//! - [`bridge`]: the connection itself, which the Interface reaches through
//!   two commands:
//!   - `core_attach { events }` attaches (launching Core if needed),
//!     completes the handshake, and returns `{ how: "attached" | "launched",
//!     hello, label? }`, where `hello` is Core's `core.hello` result and
//!     `label` names the endpoint when it is not the user's own (see
//!     [`run`]). Attaching again
//!     while connected returns the same handshake. `events` is a channel the
//!     Interface creates before calling; from then on it receives, in order,
//!     `{ kind: "notification", method, params }` for each notification Core
//!     sends and `{ kind: "lost", reason }` when the connection drops. A
//!     later attach's channel replaces it.
//!   - `core_request { method, params }` sends one Core API request and
//!     returns its result, or a [`bridge::BridgeError`].
//! - Closing and quitting (FLOW-CLOSE-AND-QUIT): closing the window hides
//!   it to the tray icon. Quit, from the tray icon's menu, emits
//!   `quit-requested` to the window, whose Interface calls
//!   `shell_quit_heard` at once, asks the user to confirm when sessions are
//!   busy, and then calls `shell_quit`. With no Interface attached to Core,
//!   or one that does not say it heard within [`QUIT_HEARD_LIMIT`], the
//!   Shell quits at once. `shell_show` shows the window for the
//!   confirmation and returns whether it was hidden; `shell_hide` puts it
//!   back when the user cancels.
//!   - [`quit`]: shutting Core down, and ending it when it will not stop.
//!   - [`instance`]: one Shell per user; a second launch shows the first's
//!     window.

pub mod bridge;
pub mod instance;
pub mod launch;
pub mod process;
pub mod quit;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tauri::ipc::Channel;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::Url;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tokio::sync::Mutex;

use bridge::{BridgeError, CoreLink, LinkEvent, LinkOptions};
use launch::CoreLocation;

/// Where the Interface's events go: the channel of the latest `core_attach`.
type Sink = Arc<std::sync::Mutex<Option<Channel<Value>>>>;

/// What the Shell keeps between commands.
struct Shell {
    /// Where Core is, or why we could not work that out.
    location: Result<CoreLocation, String>,
    /// The endpoint's label when the window attaches to a labelled endpoint
    /// instead of the user's own; told to the Interface so it can say so.
    label: Option<String>,
    /// The open connection, if any. An async mutex because attaching holds it
    /// across the launch, so two attaches never launch two Cores.
    link: Mutex<Option<Arc<CoreLink>>>,
    sink: Sink,
    /// Set once Quit has begun, so a second Quit does not start another,
    /// and the Interface does not attach (and launch Core) again meanwhile.
    quitting: AtomicBool,
    /// How many Quits the tray has passed to the Interface, and the count
    /// when the Interface last said it heard one.
    quit_asked: AtomicU64,
    quit_heard: AtomicU64,
}

/// How long the Interface has to say it heard a Quit before the Shell
/// quits without it: a window whose script is stuck must not make Quit
/// do nothing.
pub const QUIT_HEARD_LIMIT: Duration = Duration::from_secs(5);

#[tauri::command]
async fn core_attach(
    shell: State<'_, Shell>,
    events: Channel<Value>,
) -> Result<Value, BridgeError> {
    // Replace the sink before anything else, so nothing Core sends from here
    // on is lost or delivered to an Interface that no longer listens.
    *shell.sink.lock().expect("sink lock") = Some(events);
    let quitting = || {
        shell
            .quitting
            .load(Ordering::SeqCst)
            .then(|| BridgeError::unavailable("Ersa is quitting"))
    };
    // After Quit, the Interface sees its connection drop and attaches again;
    // that must never launch a new Core behind the one shutting down.
    if let Some(refused) = quitting() {
        return Err(refused);
    }
    let mut link = shell.link.lock().await;
    if let Some(refused) = quitting() {
        return Err(refused);
    }
    let reply = |how: &str, hello: &Value| {
        let mut reply = json!({ "how": how, "hello": hello });
        if let Some(label) = &shell.label {
            reply["label"] = json!(label);
        }
        reply
    };
    if let Some(open) = link.as_ref().filter(|l| !l.is_closed()) {
        return Ok(reply("attached", open.hello()));
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
    let reply = reply(how.as_str(), opened.hello());
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

/// Shows the window, for the Interface's quit confirmation; whether it was
/// hidden before.
#[tauri::command]
fn shell_show(app: AppHandle) -> bool {
    let hidden = app
        .get_webview_window(WINDOW)
        .is_some_and(|window| !window.is_visible().unwrap_or(true));
    show_window(&app);
    hidden
}

/// Hides the window again, when the user cancels a quit the window was
/// shown for (FLOW-CLOSE-AND-QUIT-R04: everything stays as it was).
#[tauri::command]
fn shell_hide(app: AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.hide();
    }
}

/// The Interface has received `quit-requested` and will answer it.
#[tauri::command]
fn shell_quit_heard(shell: State<'_, Shell>) {
    let asked = shell.quit_asked.load(Ordering::SeqCst);
    shell.quit_heard.fetch_max(asked, Ordering::SeqCst);
}

/// Quits: called by the Interface once the user has confirmed, or when
/// there was nothing to confirm.
#[tauri::command]
async fn shell_quit(app: AppHandle) -> Result<(), ()> {
    quit_now(&app).await;
    Ok(())
}

/// The window's name, as built in [`run`].
const WINDOW: &str = "main";

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Quit from the tray icon's menu. An Interface attached to Core decides
/// whether to confirm first (FLOW-CLOSE-AND-QUIT-R04); without one there is
/// no session the window could name, and the Shell quits at once.
async fn quit_requested(app: AppHandle) {
    let shell = app.state::<Shell>();
    // An attach still in progress holds the lock, maybe for a launch; the
    // window cannot have counted sessions yet, so that counts as none.
    let attached = shell
        .link
        .try_lock()
        .is_ok_and(|link| link.as_ref().is_some_and(|link| !link.is_closed()));
    if !attached {
        return quit_now(&app).await;
    }
    let mine = shell.quit_asked.fetch_add(1, Ordering::SeqCst) + 1;
    if app.emit_to(WINDOW, "quit-requested", ()).is_err() {
        return quit_now(&app).await;
    }
    tokio::time::sleep(QUIT_HEARD_LIMIT).await;
    if shell.quit_heard.load(Ordering::SeqCst) < mine {
        eprintln!("ersa: quit: the window did not answer; quitting without it");
        quit_now(&app).await;
    }
}

/// Shuts Core down and exits once it has stopped (FLOW-CLOSE-AND-QUIT-R03,
/// R06). A window attached to a labelled endpoint leaves that Core running:
/// someone else started it (FLOW-CLOSE-AND-QUIT, Development).
async fn quit_now(app: &AppHandle) {
    let shell = app.state::<Shell>();
    if shell.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.hide();
    }
    // Let go of the Interface's connection first: Core closes it anyway. An
    // attach in progress, perhaps launching Core, is let finish, so the Core
    // it launches is the one shut down below; later attaches are refused.
    let attach_limit = launch::START_TIMEOUT + bridge::PROBE_TIMEOUT;
    if let Ok(mut link) = tokio::time::timeout(attach_limit, shell.link.lock()).await {
        link.take();
    }
    if let Some(endpoint) =
        quit::endpoint_to_stop(shell.label.as_deref(), shell.location.as_ref().ok())
    {
        // Bounded as a whole, so nothing in it can keep the app from exiting.
        let limit = quit::QUIT_LIMIT + quit::KILL_WAIT * 2;
        match tokio::time::timeout(limit, quit::shut_down(endpoint, quit::QUIT_LIMIT)).await {
            Ok(Ok(stopped)) => eprintln!("ersa: quit: {stopped:?}"),
            Ok(Err(e)) => eprintln!("ersa: quit: {e}"),
            Err(_) => eprintln!("ersa: quit: Core did not stop within {limit:?}"),
        }
    }
    app.exit(0);
}

/// The tray icon: a click shows the window, and its menu shows the window
/// or quits.
fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Ersa", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Ersa", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::with_id("ersa")
        .tooltip("Ersa")
        .menu(&menu)
        // The menu is for the right button; the left one shows the window.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_window(app),
            "quit" => {
                tauri::async_runtime::spawn(quit_requested(app.clone()));
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
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
fn guard_navigation(url: &Url) -> bool {
    if is_app_origin(url) {
        return true;
    }
    if matches!(url.scheme(), "http" | "https") {
        open_in_browser(url);
    }
    false
}

/// Hands `url` to the operating system's default browser. The URL is one
/// argument to the program, never a shell command line, and is already
/// parsed as http(s).
fn open_in_browser(url: &Url) {
    let program = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program)
        .arg(url.as_str())
        .spawn();
}

/// The endpoint label a debug build attaches to instead of the user's own
/// Core, from `ERSA_ATTACH`: for developing the Interface against a replay
/// server (`ersa-core replay` serves the label `replay`) or a second Core.
/// The window never launches Core there. A release build ignores it, so
/// nothing in the environment can point the installed app at another Core.
fn attach_label() -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var("ERSA_ATTACH")
        .ok()
        .filter(|label| !label.is_empty())
}

/// Opens the window, or shows the window of the Shell already running.
/// Called by `main`.
pub fn run() {
    let label = attach_label();
    // One Shell per user (FLOW-CLOSE-AND-QUIT-R02). An endpoint that cannot
    // be claimed (another user's process holds its name) costs only that.
    let claimed = ersa_core::platform::labelled_endpoint(&instance::label_for(label.as_deref()))
        .and_then(|endpoint| tauri::async_runtime::block_on(instance::claim(&endpoint)));
    let listener = match claimed {
        Ok(instance::Instance::Running) => return,
        Ok(instance::Instance::First(listener)) => Some(listener),
        Err(e) => {
            eprintln!("ersa: cannot make sure this is the only window: {e}");
            None
        }
    };
    let location = match &label {
        Some(label) => CoreLocation::attach_only(label),
        None => CoreLocation::default_for_user(),
    };
    let shell = Shell {
        location: location.map_err(|e| format!("cannot locate Core: {e}")),
        label,
        link: Mutex::new(None),
        sink: Arc::new(std::sync::Mutex::new(None)),
        quitting: AtomicBool::new(false),
        quit_asked: AtomicU64::new(0),
        quit_heard: AtomicU64::new(0),
    };
    tauri::Builder::default()
        .manage(shell)
        .invoke_handler(tauri::generate_handler![
            core_attach,
            core_request,
            shell_show,
            shell_hide,
            shell_quit_heard,
            shell_quit
        ])
        // The window is built here rather than in tauri.conf.json so it can
        // carry the navigation guard.
        .setup(move |app| {
            WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("index.html".into()))
                .title("Ersa")
                .inner_size(1200.0, 800.0)
                .min_inner_size(640.0, 400.0)
                .on_navigation(guard_navigation)
                .build()?;
            build_tray(app.handle())?;
            if let Some(listener) = listener {
                let app = app.handle().clone();
                tauri::async_runtime::spawn(instance::serve_requests(listener, move || {
                    show_window(&app)
                }));
            }
            Ok(())
        })
        // Closing the window hides it; Core and the sessions keep running
        // (FLOW-CLOSE-AND-QUIT-R01). Only Quit ends the app.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
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
