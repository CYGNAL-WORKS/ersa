//! One Shell per user (FLOW-CLOSE-AND-QUIT-R02): the first Shell serves an
//! endpoint of its own, and a second launch finds it there, asks it to show
//! its window, and exits instead of opening another.
//!
//! The endpoint is one of the user's labelled endpoints, with the same
//! guarantees as Core's: only this user's processes can serve or reach it.

use std::io;

use ersa_core::platform::Listener;

/// The label of the window's endpoint, beside the user's own Core endpoint.
/// A debug window attached to another endpoint (`ERSA_ATTACH`) is a Shell
/// of its own, beside the user's window: its label adds that endpoint's.
pub fn label_for(attach: Option<&str>) -> String {
    match attach {
        None => "window".to_string(),
        Some(label) => format!("window-{label}"),
    }
}

/// What a launch found.
pub enum Instance {
    /// This is the first Shell: it shows its window whenever a request
    /// arrives on the listener ([`next_request`]).
    First(Listener),
    /// A Shell was already running and has been asked to show its window.
    Running,
}

/// Serves `endpoint` as the first Shell, or asks the Shell already serving
/// it to show its window.
pub async fn claim(endpoint: &str) -> io::Result<Instance> {
    match Listener::bind(endpoint).await {
        Ok(listener) => Ok(Instance::First(listener)),
        // `bind` found the first Shell by connecting to it, and a connection
        // is the request: it has already been asked to show its window.
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => Ok(Instance::Running),
        Err(e) => Err(e),
    }
}

/// Calls `show` each time a later launch asks for the window, for as long
/// as the Shell runs. A launch that lets go before it is accepted fails one
/// accept, never the endpoint: as Core's own loop does, this pauses and goes
/// on, so the endpoint stays claimed.
pub async fn serve_requests(mut listener: Listener, show: impl Fn()) {
    loop {
        // Dropping the connection at once: there is nothing to answer.
        match listener.accept().await {
            Ok(_) => show(),
            Err(e) => {
                eprintln!("ersa: a later launch could not reach this window: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}
