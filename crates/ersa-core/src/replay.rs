//! The replay server: Core serving one recorded session, with no backend
//! behind it (`ARCH-CORE`, Implementation decisions; PROTO-CORE-API-R09).
//!
//! It is Core itself, not an imitation. [`Server::bind_replay`] binds Core
//! as usual, records a project, a workspace and one session in its
//! database, and starts that session with a [`ReplayAdapter`]: instead of a
//! backend process, it feeds the recording's lines into the session's raw
//! log, on the recorded timing or all at once, from the moment a client
//! first subscribes (so an Interface that attaches later still sees the
//! session stream as it did). Everything else (the raw log,
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
use ersa_adapter::recording::TimedLine;
use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, PermissionDecision, ProcessExit, Projector,
    SessionConfig, Transport, async_trait,
};
use ersa_protocol::core_api::{Method, Project, Vcs};
use serde_json::Value;
use tokio::sync::watch;

use crate::api::ApiError;
use crate::persistence::{self, LOCAL_HOST_ID, SessionRow};
use crate::sessions::SessionError;
use crate::state::Core;
use crate::transport::MAX_PROCESS_LINE_BYTES;
use crate::workspace;

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
    // Every method is listed, so a new one cannot be added without
    // deciding whether a replay may run it: what is not known to be
    // read-only is refused.
    let message = match method {
        Method::CoreHello
        | Method::CoreStatus
        | Method::CoreShutdown
        | Method::HostList
        | Method::BackendList
        | Method::ModeList
        | Method::ProjectList
        | Method::ProjectFind
        | Method::WorkspaceList
        | Method::BranchList
        | Method::WorkspaceChanges
        | Method::SettingsGet
        | Method::SessionList
        | Method::SessionSubscribe
        | Method::SessionUnsubscribe
        | Method::SessionRawLog
        | Method::AttachmentInspect
        | Method::SectionList => return None,
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
        | Method::SessionSetArchived
        | Method::SessionArrange
        | Method::SessionDelete
        | Method::SessionSetMode => "the session is a replay of a recording and cannot change",
        Method::ProjectAdd
        | Method::ProjectSetTrust
        | Method::ProjectRemove
        | Method::WorkspaceCreate
        | Method::WorkspaceRemove
        | Method::SettingsSet
        | Method::SectionCreate
        | Method::SectionRename
        | Method::SectionRemove
        | Method::HostAdd
        | Method::HostRemove
        | Method::HostProbe
        | Method::FsList
        | Method::FsRead
        | Method::FsDiff
        | Method::TerminalOpen
        | Method::TerminalInput
        | Method::TerminalResize
        | Method::TerminalClose => "this Core is a replay server and changes nothing",
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
    // Core's own limits apply: a live Core ends a backend that writes a
    // longer line, so a replay must not serve one either.
    let lines = recording.into_lines();
    check_lines(&lines, MAX_PROCESS_LINE_BYTES)?;
    // The data directory stands in for the folder the recording was made
    // in: it exists, and nothing ever runs there.
    let path = core.config.data_dir.display().to_string();
    let project = Project {
        id: persistence::new_id("p"),
        host_id: LOCAL_HOST_ID.to_string(),
        path: path.clone(),
        name: title.clone(),
        vcs: Vcs::None,
        // A recording runs no configuration, so there is nothing to trust.
        trusted: false,
    };
    // Stored under the key `project.add` would give the folder, so
    // `project.find` answers for it as it does on a live Core.
    let key = workspace::location_key(&path);

    // The recording plays in the project's root workspace, its own folder.
    let workspace = core.store.insert_project(project.clone(), key).await?;

    let session = SessionRow {
        id: persistence::new_id("s"),
        workspace_id: workspace.id,
        host_id: LOCAL_HOST_ID.to_string(),
        backend: adapter.backend().to_string(),
        backend_session_id: None,
        mode: None,
        cwd: path.clone(),
        model: None,
        effort: None,
        // A replayed session names none, so it reports what Core states to
        // the backend then: its default permission mode.
        permission_mode: Some(ersa_adapter::BackendChoices::default_of(
            &adapter.choices().permission_modes,
        ))
        .filter(|mode| !mode.is_empty()),
        archived: false,
        deleting: None,
        pinned: false,
        section_id: None,
        last_activity_at: None,
    };
    let transport = core
        .session_transport_for(LOCAL_HOST_ID)
        .ok_or_else(|| SessionError::Internal("no local transport".into()))?;
    let config = SessionConfig {
        cwd: path,
        permission_mode: String::new(),
        model: None,
        effort: None,
        project_trusted: false,
        project_path: String::new(),
        // A recording names its session itself, as it did when recorded.
        backend_session_id: None,
        extra: Value::Null,
    };
    core.sessions
        .create_with(
            session,
            Some(title),
            Arc::new(ReplayAdapter {
                inner: adapter,
                lines: Arc::new(lines),
                pace,
                audience: core.replay_audience(),
            }),
            transport,
            config,
            backend_version,
        )
        .await?;
    Ok(())
}

/// Refuses a recording with a backend line longer than `max_bytes`, the
/// limit a live Core reads a backend's stdout under.
fn check_lines(lines: &[TimedLine], max_bytes: usize) -> Result<(), SessionError> {
    let too_long =
        |l: &TimedLine| l.dir == ersa_adapter::Direction::Backend && l.line.len() > max_bytes;
    match lines.iter().position(too_long) {
        Some(at) => Err(SessionError::InvalidState(format!(
            "line {at} of the recording is longer than Core's limit of {max_bytes} bytes"
        ))),
        None => Ok(()),
    }
}

/// Raised once a client has subscribed to the replayed session: the
/// replay's clock starts then. Called on every successful
/// `session.subscribe`; does nothing outside a replay server.
pub(crate) fn subscribed(core: &Core) {
    if core.config.replay {
        core.replay_audience_sender().send_replace(true);
    }
}

/// A backend's adapter without its session records: the record methods
/// keep their defaults (no history, no summary). A replay server registers
/// its recorded backend this way, so nothing it does reads the user's own
/// records.
pub struct Recordless(Arc<dyn Adapter>);

impl Recordless {
    pub fn new(adapter: Arc<dyn Adapter>) -> Self {
        Recordless(adapter)
    }
}

#[async_trait]
impl Adapter for Recordless {
    fn backend(&self) -> &str {
        self.0.backend()
    }

    async fn start(
        &self,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        sink: Arc<dyn AdapterSink>,
    ) -> Result<Box<dyn AdapterSession>, AdapterError> {
        self.0.start(transport, config, sink).await
    }

    fn set_environment(&self, spec: &mut ersa_adapter::ProcessSpec) {
        self.0.set_environment(spec)
    }

    fn tested_versions(&self) -> Option<(&'static str, &'static str)> {
        self.0.tested_versions()
    }

    fn label(&self) -> &str {
        self.0.label()
    }

    fn choices(&self) -> ersa_adapter::BackendChoices {
        self.0.choices()
    }

    fn projector(&self) -> Box<dyn Projector> {
        self.0.projector()
    }
}

/// Feeds a recording into a session's raw log in place of a backend.
pub struct ReplayAdapter {
    inner: Arc<dyn Adapter>,
    lines: Arc<Vec<TimedLine>>,
    pace: Pace,
    /// Rises when a client first subscribes; nothing is fed before.
    audience: watch::Receiver<bool>,
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
        let mut audience = self.audience.clone();
        tokio::spawn(async move {
            // Held until someone is watching, so the recorded timing is what
            // they see rather than a backlog of everything played so far.
            // `Err` means Core is gone; the stop below ends the task then.
            tokio::select! {
                _ = audience.wait_for(|watched| *watched) => {}
                _ = stopped.wait_for(|stop| *stop) => {}
            }
            let begun = tokio::time::Instant::now();
            for TimedLine {
                at_ms: at,
                dir,
                line,
            } in lines.iter()
            {
                // At an instant pace nothing else awaits here: yield now and
                // then, so a long recording does not hold a worker thread
                // the sockets need.
                if pace == Pace::Instant {
                    tokio::task::yield_now().await;
                }
                if *stopped.borrow() {
                    break;
                }
                if pace == Pace::Realtime {
                    // The recording caps its times, but a time past what the
                    // clock can hold must still not panic this task, which
                    // would leave the session's exit unreported.
                    let due = begun
                        .checked_add(Duration::from_millis(*at))
                        .unwrap_or(begun);
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

    fn line(at_ms: u64, dir: Direction, line: &str) -> TimedLine {
        TimedLine {
            at_ms,
            dir,
            line: line.into(),
        }
    }

    /// A replay adapter over three lines, and the flag that says a client
    /// subscribed, already raised.
    fn adapter(pace: Pace) -> (ReplayAdapter, watch::Sender<bool>) {
        let (audience, watching) = watch::channel(true);
        let adapter = ReplayAdapter {
            inner: Arc::new(NoBackend),
            lines: Arc::new(vec![
                line(0, Direction::Host, "a"),
                line(1500, Direction::Backend, "b"),
                line(4000, Direction::Backend, "c"),
            ]),
            pace,
            audience: watching,
        };
        (adapter, audience)
    }

    // Tokio's clock is paused, so the sleeps take no real time and the
    // arrival times are exact.
    #[tokio::test(start_paused = true)]
    async fn realtime_feeds_each_line_on_its_recorded_time_and_stop_ends_it() {
        let sink = Timed::new();
        let transport: Arc<dyn Transport> = Arc::new(ersa_adapter::LocalTransport);
        let (adapter, _audience) = adapter(Pace::Realtime);
        let session = adapter
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
    async fn the_clock_starts_when_a_client_first_subscribes() {
        let sink = Timed::new();
        let transport: Arc<dyn Transport> = Arc::new(ersa_adapter::LocalTransport);
        let (adapter, audience) = adapter(Pace::Realtime);
        audience.send_replace(false);
        let session = adapter
            .start(transport, SessionConfig::default(), sink.clone())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(10)).await;
        assert!(
            sink.lines.lock().unwrap().is_empty(),
            "nothing before a subscriber"
        );
        audience.send_replace(true);
        tokio::time::sleep(Duration::from_secs(5)).await;
        let arrived = sink.lines.lock().unwrap().clone();
        assert_eq!(
            arrived,
            [
                (Duration::from_millis(10_000), "a".to_string()),
                (Duration::from_millis(11_500), "b".to_string()),
                (Duration::from_millis(14_000), "c".to_string()),
            ]
        );
        session.stop().await.unwrap();
    }

    #[test]
    fn a_line_over_the_limit_is_refused() {
        let lines = [
            line(0, Direction::Backend, "1234"),
            line(1, Direction::Backend, "12345"),
        ];
        assert!(check_lines(&lines[..1], 4).is_ok());
        // The limit is on what a backend writes, as in a live Core.
        assert!(check_lines(&[line(0, Direction::Host, "12345")], 4).is_ok());
        let refused = check_lines(&lines, 4).unwrap_err();
        assert!(
            matches!(&refused, SessionError::InvalidState(m) if m.contains("line 1")),
            "{refused}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_stop_partway_ends_the_replay_without_the_rest() {
        let sink = Timed::new();
        let transport: Arc<dyn Transport> = Arc::new(ersa_adapter::LocalTransport);
        let (adapter, _audience) = adapter(Pace::Realtime);
        let session = adapter
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
