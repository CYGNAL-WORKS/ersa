//! The replay server: Core serving one recorded session, with no backend
//! behind it (`ARCH-CORE`, Implementation decisions; PROTO-CORE-API-R09).
//!
//! It is Core itself, not an imitation. [`Server::bind_replay`] binds Core
//! as usual, records a project, a workspace and one session in its
//! database, and starts that session with a [`ReplayAdapter`]: instead of a
//! backend process, it feeds the recording's lines into the session's raw
//! log, on the recorded timing or all at once. Everything else (the raw log,
//! projection, subscriptions, tiles, every read-only method) is Core's own
//! code, which is what makes the replay answer exactly as Core would.
//!
//! What would change anything is refused with `invalid_state` (see
//! [`refusal`]): there is no backend to send to, and a replay must leave the
//! user's projects alone.
//!
//! [`Server::bind_replay`]: crate::Server::bind_replay

use std::sync::Arc;
use std::time::Duration;

// Re-exported for the binary, which builds a [`Replay`] from a file.
pub use ersa_adapter::recording::Recording;
use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, PermissionDecision, ProcessExit, Projector,
    SessionConfig, Transport, async_trait,
};
use ersa_protocol::core_api::{Method, Project, Session, Tile, Vcs, Workspace, WorkspaceState};
use ersa_protocol::events::LifecycleState;
use serde_json::Value;
use tokio::sync::watch;

use crate::api::ApiError;
use crate::persistence::{self, LOCAL_HOST_ID};
use crate::sessions::SessionError;
use crate::state::Core;

/// How fast a replay feeds its lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    /// Each line when it crossed in the recording, counted from the start,
    /// so the Interface sees streaming as it really arrives.
    Realtime,
    /// Every line at once.
    Instant,
}

/// What a replay server serves.
pub struct Replay {
    /// The recorded backend's adapter. Only its name and projector are
    /// used; it never starts anything.
    pub adapter: Arc<dyn Adapter>,
    pub recording: Recording,
    /// The session's title.
    pub title: String,
    /// The backend version Core records the session as started with, as
    /// detection would have found it.
    pub backend_version: String,
    pub pace: Pace,
}

/// The answer a replay server gives `method` instead of running it, when
/// running it would change something. `None` for a method it runs as Core.
pub(crate) fn refusal(core: &Core, method: Method) -> Option<ApiError> {
    if !core.config.replay {
        return None;
    }
    let message = match method {
        Method::SessionCreate
        | Method::SessionStart
        | Method::SessionStop
        | Method::SessionResume
        | Method::SessionRemove
        | Method::SessionSend
        | Method::SessionInterrupt
        | Method::SessionDecidePermission
        | Method::SessionAnswerQuestion
        | Method::SessionSetTitle
        | Method::SessionSetMode
        | Method::SessionReproject => "the session is a replay of a recording and cannot change",
        Method::ProjectAdd
        | Method::ProjectRemove
        | Method::WorkspaceCreate
        | Method::WorkspaceRemove
        | Method::SettingsSet => "this Core is a replay server and changes nothing",
        _ => return None,
    };
    Some(ApiError::invalid_state(format!(
        "{}: {message}",
        method.name()
    )))
}

/// Records the replay's project, workspace and session, and starts the
/// session. Called once, as the replay server binds.
pub(crate) async fn start(core: &Core, replay: Replay) -> Result<(), SessionError> {
    let Replay {
        adapter,
        recording,
        title,
        backend_version,
        pace,
    } = replay;
    // The data directory stands in for the folder the recording was made
    // in: it exists, and nothing ever runs there.
    let path = core.config.data_dir.display().to_string();
    let project = Project {
        id: persistence::new_id("p"),
        host_id: LOCAL_HOST_ID.to_string(),
        path: path.clone(),
        name: title.clone(),
        vcs: Vcs::None,
    };
    core.store
        .insert_project(project.clone(), path.clone())
        .await?;
    let workspace = Workspace {
        id: persistence::new_id("w"),
        project_id: project.id,
        path: path.clone(),
        vcs_ref: String::new(),
        state: WorkspaceState::Ready,
    };
    core.store.insert_workspace(workspace.clone()).await?;

    let session = Session {
        id: persistence::new_id("s"),
        workspace_id: workspace.id,
        host_id: LOCAL_HOST_ID.to_string(),
        backend: adapter.backend().to_string(),
        mode: None,
        title: Some(title),
        state: LifecycleState::Starting,
        created_at: persistence::now_rfc3339(),
        last_event_seq: None,
        tile: Tile::starting(),
    };
    let transport = core
        .session_transport_for(LOCAL_HOST_ID)
        .ok_or_else(|| SessionError::Internal("no local transport".into()))?;
    let config = SessionConfig {
        cwd: path,
        permission_mode: String::new(),
        model: None,
        extra: Value::Null,
    };
    let lines = recording
        .timed_lines()
        .map_err(|e| SessionError::Internal(format!("the recording is unreadable: {e}")))?;
    core.sessions
        .create_with(
            session,
            Arc::new(ReplayAdapter {
                inner: adapter,
                lines: Arc::new(lines),
                pace,
            }),
            transport,
            config,
            backend_version,
        )
        .await?;
    Ok(())
}

/// Feeds a recording into a session's raw log in place of a backend.
pub struct ReplayAdapter {
    inner: Arc<dyn Adapter>,
    /// Each line with the milliseconds after the start at which it crossed.
    lines: Arc<Vec<(u64, ersa_adapter::Direction, String)>>,
    pace: Pace,
}

#[async_trait]
impl Adapter for ReplayAdapter {
    fn backend(&self) -> &str {
        self.inner.backend()
    }

    /// Ready at once, as a backend is that waits for its first message; the
    /// lines follow in the background. The "process" then stays up, idle,
    /// until the session is stopped, as the recorded one did until its
    /// host closed it.
    async fn start(
        &self,
        _: Arc<dyn Transport>,
        _: SessionConfig,
        sink: Arc<dyn AdapterSink>,
    ) -> Result<Box<dyn AdapterSession>, AdapterError> {
        let (stop, mut stopped) = watch::channel(false);
        let (done, finished) = watch::channel(false);
        let lines = Arc::clone(&self.lines);
        let pace = self.pace;
        tokio::spawn(async move {
            let begun = tokio::time::Instant::now();
            for (at, dir, line) in lines.iter() {
                if pace == Pace::Realtime {
                    let due = begun + Duration::from_millis(*at);
                    tokio::select! {
                        () = tokio::time::sleep_until(due) => {}
                        _ = stopped.wait_for(|stop| *stop) => break,
                    }
                }
                if *stopped.borrow() {
                    break;
                }
                sink.record(*dir, line.clone());
            }
            // `Err` means the session is gone without a stop: end anyway.
            let _ = stopped.wait_for(|stop| *stop).await;
            sink.process_exited(Ok(ProcessExit { code: Some(0) }));
            done.send_replace(true);
        });
        Ok(Box::new(ReplaySession { stop, finished }))
    }

    fn projector(&self) -> Box<dyn Projector> {
        self.inner.projector()
    }
}

/// A replayed session: it can only be stopped.
struct ReplaySession {
    stop: watch::Sender<bool>,
    finished: watch::Receiver<bool>,
}

fn is_a_replay() -> AdapterError {
    AdapterError::Protocol("the session is a replay of a recording".into())
}

#[async_trait]
impl AdapterSession for ReplaySession {
    async fn send_user(&self, _: &str) -> Result<(), AdapterError> {
        Err(is_a_replay())
    }

    async fn decide_permission(&self, _: &str, _: PermissionDecision) -> Result<(), AdapterError> {
        Err(is_a_replay())
    }

    async fn interrupt(&self) -> Result<(), AdapterError> {
        Err(is_a_replay())
    }

    /// Ends the replay, wherever it was, once its exit is reported.
    async fn stop(&self) -> Result<(), AdapterError> {
        self.stop.send_replace(true);
        let mut finished = self.finished.clone();
        // `Err` means the feeding task is gone, so it has finished too.
        let _ = finished.wait_for(|done| *done).await;
        Ok(())
    }
}

impl Drop for ReplaySession {
    /// Like a backend process, the replay ends when nothing holds it.
    fn drop(&mut self) {
        self.stop.send_replace(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ersa_adapter::{Direction, Projected, RawLine};
    use std::io;
    use std::sync::Mutex;

    /// A sink that notes when (on Tokio's clock) each line arrived.
    struct Timed {
        epoch: tokio::time::Instant,
        lines: Mutex<Vec<(Duration, String)>>,
        exited: Mutex<bool>,
    }

    impl Timed {
        fn new() -> Arc<Timed> {
            Arc::new(Timed {
                epoch: tokio::time::Instant::now(),
                lines: Mutex::default(),
                exited: Mutex::default(),
            })
        }
    }

    impl AdapterSink for Timed {
        fn record(&self, _: Direction, line: String) {
            let at = tokio::time::Instant::now() - self.epoch;
            self.lines.lock().unwrap().push((at, line));
        }
        fn stderr_line(&self, _: String) {}
        fn process_exited(&self, _: io::Result<ProcessExit>) {
            *self.exited.lock().unwrap() = true;
        }
    }

    struct NoBackend;

    #[async_trait]
    impl Adapter for NoBackend {
        fn backend(&self) -> &str {
            "none"
        }
        async fn start(
            &self,
            _: Arc<dyn Transport>,
            _: SessionConfig,
            _: Arc<dyn AdapterSink>,
        ) -> Result<Box<dyn AdapterSession>, AdapterError> {
            unreachable!("a replay never starts its backend")
        }
        fn projector(&self) -> Box<dyn Projector> {
            struct Nothing;
            impl Projector for Nothing {
                fn version(&self) -> u32 {
                    0
                }
                fn project(&mut self, _: &RawLine) -> Vec<Projected> {
                    Vec::new()
                }
            }
            Box::new(Nothing)
        }
    }

    fn adapter(pace: Pace) -> ReplayAdapter {
        ReplayAdapter {
            inner: Arc::new(NoBackend),
            lines: Arc::new(vec![
                (0, Direction::Host, "a".into()),
                (1500, Direction::Backend, "b".into()),
                (4000, Direction::Backend, "c".into()),
            ]),
            pace,
        }
    }

    // Tokio's clock is paused, so the sleeps take no real time and the
    // arrival times are exact.
    #[tokio::test(start_paused = true)]
    async fn realtime_feeds_each_line_on_its_recorded_time_and_stop_ends_it() {
        let sink = Timed::new();
        let transport: Arc<dyn Transport> = Arc::new(ersa_adapter::LocalTransport);
        let session = adapter(Pace::Realtime)
            .start(transport, SessionConfig::default(), sink.clone())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
        let arrived = sink.lines.lock().unwrap().clone();
        assert_eq!(
            arrived,
            [
                (Duration::from_millis(0), "a".to_string()),
                (Duration::from_millis(1500), "b".to_string()),
                (Duration::from_millis(4000), "c".to_string()),
            ]
        );
        // The replay stays up until it is stopped.
        assert!(!*sink.exited.lock().unwrap());
        session.stop().await.unwrap();
        assert!(*sink.exited.lock().unwrap());
        assert!(matches!(
            session.send_user("hi").await,
            Err(AdapterError::Protocol(_))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn a_stop_partway_ends_the_replay_without_the_rest() {
        let sink = Timed::new();
        let transport: Arc<dyn Transport> = Arc::new(ersa_adapter::LocalTransport);
        let session = adapter(Pace::Realtime)
            .start(transport, SessionConfig::default(), sink.clone())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
        session.stop().await.unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
        let lines: Vec<String> = sink
            .lines
            .lock()
            .unwrap()
            .iter()
            .map(|(_, l)| l.clone())
            .collect();
        assert_eq!(lines, ["a", "b"]);
        assert!(*sink.exited.lock().unwrap());
    }
}
