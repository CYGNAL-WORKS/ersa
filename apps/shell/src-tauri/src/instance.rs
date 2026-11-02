//! One Shell per user (FLOW-CLOSE-AND-QUIT-R02): the first Shell serves an
//! endpoint of its own, and a second launch finds it there, asks it to show
//! its window, and exits instead of opening another.
//!
//! The endpoint is one of the user's labelled endpoints, with the same
//! guarantees as Core's: only this user's processes can serve or reach it.

use std::io;

use ersa_core::platform::Listener;
use tokio::io::AsyncWriteExt;

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
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
            // `bind` found the first Shell by connecting to it, and a
            // connection is the request, so it has already been asked. Ask
            // once more in words, for a reader; that it fails while the
            // first Shell takes the next client changes nothing.
            if let Ok(mut stream) = ersa_core::connect(endpoint).await {
                let _ = stream.write_all(b"show\n").await;
                let _ = stream.flush().await;
            }
            Ok(Instance::Running)
        }
        Err(e) => Err(e),
    }
}

/// Waits for the next launch to ask for the window.
pub async fn next_request(listener: &mut Listener) -> io::Result<()> {
    // Dropping the connection at once: there is nothing to answer.
    listener.accept().await.map(drop)
}
