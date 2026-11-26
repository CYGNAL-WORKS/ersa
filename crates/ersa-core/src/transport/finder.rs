//! The transport a backend's commands start through on the local host: it
//! looks for the command along the search path as it is now, then starts
//! what it found with that path (PROTO-CORE-API, Finding a backend).

use std::io;
use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::{Process, ProcessSpec, Transport};
use tokio::sync::Semaphore;

use crate::search_path::{self, StoredEnvironment};

/// How many looks may be under way at once, those left running past their
/// limit included. A look that reaches a share that stopped answering keeps
/// its thread until the file system answers, so this bounds the threads
/// such shares can hold; looks of folders that answer take milliseconds.
pub const LOOKS: usize = 4;

/// How long a look for a backend's command may take before the backend is
/// `error` (PROTO-CORE-API-R151).
pub const LOOK_LIMIT: Duration = Duration::from_secs(10);

/// Wraps the transport a backend's commands start through. Before each
/// start it composes the search path (the path Core was started with, then
/// the stored system and user Path) and looks for the command along it,
/// and it starts what it found by its full path with that search path as
/// the process's own `PATH`, since a command can be a small script that
/// looks along its own path for the program that runs it
/// (PROTO-CORE-API-R148).
///
/// A command found nowhere fails with `NotFound` and nothing is started,
/// which detection reads as `missing`. A look past its limit fails with
/// `TimedOut`, and a command found that cannot be started fails with any
/// kind but `NotFound`, both of which detection reads as `error`.
pub struct Finder {
    inner: Arc<dyn Transport>,
    stored: Arc<dyn StoredEnvironment>,
    limit: Duration,
    /// The places for looks ([`LOOKS`]). Each look holds one on its thread
    /// until its file system calls return, past its limit too.
    places: Arc<Semaphore>,
}

impl Finder {
    pub fn new(
        inner: Arc<dyn Transport>,
        stored: Arc<dyn StoredEnvironment>,
        limit: Duration,
        places: Arc<Semaphore>,
    ) -> Finder {
        Finder {
            inner,
            stored,
            limit,
            places,
        }
    }

    /// Looks for `spec`'s program: the file found and the search path to
    /// give it, within the limit.
    async fn look(&self, spec: &ProcessSpec) -> io::Result<(String, Option<String>)> {
        let program = spec.program.clone();
        // The path Core was started with, or the one the adapter gave.
        let inherited = spec
            .env
            .get("PATH")
            .cloned()
            .or_else(|| std::env::var("PATH").ok());
        let stored = Arc::clone(&self.stored);
        let places = Arc::clone(&self.places);
        let look = async move {
            // A place, then the look on a thread of its own: reading the
            // registry and asking folders whether a file is there both
            // block, and a folder on a share that stopped answering blocks
            // for as long as the share does.
            let place = places
                .acquire_owned()
                .await
                .map_err(|_| io::Error::other("Core is shutting down"))?;
            tokio::task::spawn_blocking(move || {
                let _place = place;
                let stored_paths = stored.paths().unwrap_or_else(|e| {
                    // Still look along the path Core was started with.
                    tracing::warn!(error = %e, "could not read the stored search path");
                    Vec::new()
                });
                let path = search_path::compose(
                    inherited.as_deref(),
                    stored.as_ref(),
                    &stored_paths,
                    &own_variable,
                );
                let found = search_path::find_program(&program, path.as_deref());
                (found, path)
            })
            .await
            .map_err(io::Error::other)
        };
        let (found, path) = tokio::time::timeout(self.limit, look).await.map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "looking for {} did not end within {:?}",
                    spec.program, self.limit
                ),
            )
        })??;
        match found {
            Some(found) => Ok((found.display().to_string(), path)),
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} was not found on the search path", spec.program),
            )),
        }
    }
}

/// A variable in Core's own environment. A name no variable can have is
/// none, rather than a question to the system.
fn own_variable(name: &str) -> Option<String> {
    if name.contains(['=', '\0']) {
        return None;
    }
    std::env::var(name).ok()
}

#[ersa_adapter::async_trait]
impl Transport for Finder {
    async fn spawn(&self, mut spec: ProcessSpec) -> io::Result<Box<dyn Process>> {
        let (program, path) = self.look(&spec).await?;
        spec.program = program;
        if let Some(path) = path {
            spec.env.insert("PATH".to_string(), path);
        }
        self.inner.spawn(spec).await.map_err(|e| {
            // The command was found, so whatever stops it starting is not
            // its absence: `error`, not `missing`.
            if e.kind() == io::ErrorKind::NotFound {
                io::Error::other(format!("the command was found and could not start: {e}"))
            } else {
                e
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search_path::SystemStoredEnvironment;

    /// Fails every start as a file that vanished between the look and the
    /// start would.
    struct Vanishing;

    #[ersa_adapter::async_trait]
    impl Transport for Vanishing {
        async fn spawn(&self, _: ProcessSpec) -> io::Result<Box<dyn Process>> {
            Err(io::Error::new(io::ErrorKind::NotFound, "gone"))
        }
    }

    fn finder(inner: Arc<dyn Transport>) -> Finder {
        Finder::new(
            inner,
            Arc::new(SystemStoredEnvironment),
            LOOK_LIMIT,
            Arc::new(Semaphore::new(LOOKS)),
        )
    }

    /// The test binary itself, a file that is surely there.
    fn present() -> String {
        std::env::current_exe().unwrap().display().to_string()
    }

    #[tokio::test]
    async fn a_command_found_that_cannot_start_is_not_reported_missing() {
        let spec = ProcessSpec {
            program: present(),
            ..ProcessSpec::default()
        };
        let error = finder(Arc::new(Vanishing)).spawn(spec).await.err().unwrap();
        assert_ne!(error.kind(), io::ErrorKind::NotFound, "{error}");
    }

    #[tokio::test]
    async fn a_command_found_nowhere_is_not_found_and_nothing_starts() {
        let spec = ProcessSpec {
            program: "ersa-no-such-program-xyz".into(),
            ..ProcessSpec::default()
        };
        // `Vanishing` would answer NotFound too; a different kind shows
        // the inner transport is never asked.
        struct Refuses;
        #[ersa_adapter::async_trait]
        impl Transport for Refuses {
            async fn spawn(&self, _: ProcessSpec) -> io::Result<Box<dyn Process>> {
                panic!("nothing is started for a command found nowhere")
            }
        }
        let error = finder(Arc::new(Refuses)).spawn(spec).await.err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error}");
    }
}
