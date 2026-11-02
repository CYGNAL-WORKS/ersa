//! Quit (FLOW-CLOSE-AND-QUIT): ask Core to shut down, wait for its process
//! to exit, and end it, with what it started, when it has not within the
//! limit. Closing the window is not this; it only hides the window.

use std::io;
use std::time::Duration;

use crate::bridge::{CoreLink, LinkOptions};
use crate::process::CoreProcess;

/// How long Core has to stop after a quit before the Shell ends it
/// (FLOW-CLOSE-AND-QUIT-R06).
pub const QUIT_LIMIT: Duration = Duration::from_secs(10);

/// How long a process that was ended may take to be gone.
const KILL_WAIT: Duration = Duration::from_secs(5);

/// How Core stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// Nothing served the endpoint.
    NotRunning,
    /// Core shut down when asked; `pid` was its process.
    Exited { pid: u32 },
    /// Core had not stopped within the limit, and was ended.
    Killed { pid: u32 },
}

#[derive(Debug, thiserror::Error)]
pub enum QuitError {
    #[error("cannot find the Core serving {endpoint}: {source}")]
    Find { endpoint: String, source: io::Error },
    /// Whoever serves the endpoint is not this user's Core: never ended.
    #[error("refusing to stop {endpoint}: {source}")]
    NotOurs { endpoint: String, source: io::Error },
    #[error("cannot end Core (process {pid}): {source}")]
    Kill { pid: u32, source: io::Error },
    #[error("Core (process {pid}) is still running after it was ended")]
    StillRunning { pid: u32 },
}

/// Asks the Core serving `endpoint` to shut down (`core.shutdown`) and
/// returns once its process has exited (FLOW-CLOSE-AND-QUIT-R03). A Core
/// still running `limit` after the call is ended with everything it started
/// (R06). Core stops its sessions itself as part of shutting down (R05).
pub async fn shut_down(endpoint: &str, limit: Duration) -> Result<Stopped, QuitError> {
    let deadline = tokio::time::Instant::now() + limit;
    let core = CoreProcess::serving(endpoint)
        .await
        .map_err(|source| QuitError::Find {
            endpoint: endpoint.to_string(),
            source,
        })?;
    let Some(core) = core else {
        return Ok(Stopped::NotRunning);
    };
    let pid = core.pid();

    // The verified client checks that this user serves the endpoint; only
    // then may the process found above be ended. One that answers nothing
    // runs out the limit here and is ended below.
    let mut verified = false;
    let ask = async {
        let stream = match ersa_core::connect(endpoint).await {
            Ok(stream) => stream,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Err(e),
            // Gone since it was found, or too busy to take a client: the wait
            // below tells which.
            Err(_) => return Ok(()),
        };
        verified = true;
        let options = LinkOptions {
            probe_endpoint: None,
            timeout: Some(limit),
        };
        if let Ok(link) = CoreLink::open(stream, options, |_| {}).await {
            // Core answers and then closes; either way, its process exiting
            // is what counts.
            let _ = link.request("core.shutdown", None).await;
        }
        Ok(())
    };
    let refused = match tokio::time::timeout_at(deadline, ask).await {
        Ok(Err(source)) => Some(source),
        Ok(Ok(())) | Err(_) => None,
    };
    if let Some(source) = refused {
        return Err(QuitError::NotOurs {
            endpoint: endpoint.to_string(),
            source,
        });
    }

    let left = deadline.saturating_duration_since(tokio::time::Instant::now());
    if core.wait_exit(left).await {
        return Ok(Stopped::Exited { pid });
    }
    if !verified {
        return Err(QuitError::NotOurs {
            endpoint: endpoint.to_string(),
            source: io::Error::new(
                io::ErrorKind::TimedOut,
                "it did not take a client to check whose it is",
            ),
        });
    }
    core.kill_tree()
        .map_err(|source| QuitError::Kill { pid, source })?;
    if core.wait_exit(KILL_WAIT).await {
        Ok(Stopped::Killed { pid })
    } else {
        Err(QuitError::StillRunning { pid })
    }
}
