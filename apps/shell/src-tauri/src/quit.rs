//! Quit (FLOW-CLOSE-AND-QUIT): ask Core to shut down, wait for its process
//! to exit, and end it, with what it started, when it has not within the
//! limit. Closing the window is not this; it only hides the window.

use std::io;
use std::time::Duration;

use crate::bridge::{CoreLink, LinkOptions};
use crate::launch::CoreLocation;
use crate::process::CoreProcess;

/// How long Core has to stop after a quit before the Shell ends it
/// (FLOW-CLOSE-AND-QUIT-R06).
pub const QUIT_LIMIT: Duration = Duration::from_secs(10);

/// How long a process that was ended may take to be gone.
pub const KILL_WAIT: Duration = Duration::from_secs(5);

/// How Core stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// Nothing served the endpoint.
    NotRunning,
    /// Core shut down when asked; `pid` was its process.
    Exited { pid: u32 },
    /// Core had not stopped within the limit, and was ended.
    Killed { pid: u32 },
    /// Core stopped serving when asked; its process could not be found to
    /// watch (FLOW-CLOSE-AND-QUIT, inbox: macOS).
    Unwatched,
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

/// The endpoint whose Core a quit stops: the user's own. A window attached
/// to a labelled endpoint (`attach_label`, from `ERSA_ATTACH`) quits without
/// stopping that Core, which someone else started (FLOW-CLOSE-AND-QUIT,
/// Development); `None` too when Core could not be located.
pub fn endpoint_to_stop<'a>(
    attach_label: Option<&str>,
    location: Option<&'a CoreLocation>,
) -> Option<&'a str> {
    match attach_label {
        Some(_) => None,
        None => location.map(|l| l.endpoint.as_str()),
    }
}

/// Asks the Core serving `endpoint` to shut down (`core.shutdown`) and
/// returns once its process has exited (FLOW-CLOSE-AND-QUIT-R03). A Core
/// still running `limit` after the call is ended with everything it started
/// (R06). Core stops its sessions itself as part of shutting down (R05).
pub async fn shut_down(endpoint: &str, limit: Duration) -> Result<Stopped, QuitError> {
    let deadline = tokio::time::Instant::now() + limit;
    // Core is asked to stop even when its process cannot be found (on macOS,
    // say); then only the fallback of ending it is lost.
    let (core, unfound) = match CoreProcess::serving(endpoint).await {
        Ok(None) => return Ok(Stopped::NotRunning),
        Ok(Some(core)) => (Some(core), None),
        // Another user's process: never asked, never ended.
        Err(source) if source.kind() == io::ErrorKind::PermissionDenied => {
            return Err(QuitError::NotOurs {
                endpoint: endpoint.to_string(),
                source,
            });
        }
        Err(e) => (None, Some(e)),
    };

    // A Core that answers nothing, or takes no client, runs out the limit
    // here and is ended below.
    let ask = async {
        let stream = match ersa_core::connect(endpoint).await {
            Ok(stream) => stream,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Err(e),
            // Gone since it was found, or too busy to take a client: the wait
            // below tells which.
            Err(_) => return Ok(()),
        };
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
    let Some(core) = core else {
        // Without the process, stopping is seen as the endpoint going away.
        return if unserved_within(endpoint, left).await {
            Ok(Stopped::Unwatched)
        } else {
            Err(QuitError::Find {
                endpoint: endpoint.to_string(),
                source: unfound.unwrap_or_else(|| io::Error::other("no process")),
            })
        };
    };
    let pid = core.pid();
    if core.wait_exit(left).await {
        return Ok(Stopped::Exited { pid });
    }
    core.kill_tree()
        .map_err(|source| QuitError::Kill { pid, source })?;
    if core.wait_exit(KILL_WAIT).await {
        Ok(Stopped::Killed { pid })
    } else {
        Err(QuitError::StillRunning { pid })
    }
}

/// Whether nothing serves `endpoint` any more, within `limit`.
async fn unserved_within(endpoint: &str, limit: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        match ersa_core::connect(endpoint).await {
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) =>
            {
                return true;
            }
            _ if tokio::time::Instant::now() >= deadline => return false,
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // FLOW-CLOSE-AND-QUIT, Development.
    #[test]
    fn a_window_attached_by_label_quits_without_stopping_that_core() {
        let location = CoreLocation::attach_only("replay").expect("a location");
        assert_eq!(endpoint_to_stop(Some("replay"), Some(&location)), None);
        assert_eq!(
            endpoint_to_stop(None, Some(&location)),
            Some(location.endpoint.as_str())
        );
        assert_eq!(endpoint_to_stop(None, None), None);
    }
}
