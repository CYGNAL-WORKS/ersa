//! Session manager: the lifecycle of every session (created, starting,
//! running, waiting on user, idle, ended). Routes user input and permission
//! decisions to adapters, and what crosses a backend's stdio to the session's
//! projection.
//!
//! A session's history is the backend's own session record (ADR 0017).
//! Nothing projected is stored: each session Core shows has an open
//! [`View`] in memory, made from the record when the session is opened and
//! extended by the live stream while Core runs its backend.
//!
//! One running session is three tasks and a sink:
//!
//! - The **sink** ([`CoreSink`]) is what the adapter records lines into. It
//!   stamps each line with the next index and the time and queues it, never
//!   blocking, as `AdapterSink` requires, together with Core's own lifecycle
//!   facts. Stamping under one lock keeps them in the order they happened.
//! - The **pipeline** task takes what was queued in batches: lines go to the
//!   in-memory raw log for the developer pane, facts to the app log, and both
//!   through the view, whose new events it hands to subscribers.
//! - The **session** task starts the backend through its adapter, waits for
//!   the process to end, then closes the sink, lets the pipeline drain, and
//!   marks the session no longer live.
//! - Each **subscription** has a task of its own that replays the events a
//!   client asked for and then forwards live ones. The pipeline never waits
//!   for a client: it offers each event to a subscription's queue and drops a
//!   subscription whose queue is full, and that subscription's connection is
//!   closed, so the client knows to reconnect and resubscribe.
//!
//! When views are opened: a session whose backend Core starts gets a view as
//! it starts. A session not running in this Core gets one when a client first
//! subscribes, and a fresh one when a client subscribes from the start
//! (`from_seq` 0) while nobody else is subscribed, so turns taken elsewhere
//! (in the terminal, say) show when the session is opened again. A client
//! resubscribing from where it left off keeps the view it was reading.
//!
//! Disconnecting a client affects only its subscriptions; a session runs
//! until it is stopped, its backend exits, or Core shuts down
//! (PROTO-CORE-API-R03, ARCH-CORE-R08).

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, Direction, PermissionDecision, ProcessExit,
    RawLine, RecordEvent, SessionConfig, Transport,
};
use ersa_protocol::core_api::rpc::Notification;
use ersa_protocol::core_api::{Session, SessionEventParams, SessionTileParams, Tile};
use ersa_protocol::events::{Envelope, ErrorCode, ErrorEvent, Event, LifecycleState};
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::task::AbortHandle;

use crate::app_log::{AppLog, Facts, LifecycleFact};
use crate::persistence::{self, SessionRow, Store, now_rfc3339};
use crate::projection::View;
use crate::raw_log::RawLog;

/// How many events may wait for one subscriber before it counts as unable
/// to keep up. Streaming text produces many small events, so this is
/// generous; a client further behind than this is disconnected.
const SUBSCRIBER_QUEUE: usize = 4096;

/// The most raw lines the pipeline projects in one go, before it hands the
/// events on.
const BATCH: usize = 256;

/// How long `session.stop` waits for the backend to end and its last lines
/// to be projected. The Claude Code adapter kills after three seconds; this
/// is the backstop for an adapter that never returns.
const STOP_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a request waits for a session that is still starting to become
/// ready. Starting is bounded by the adapter's own handshake timeout; this
/// is the backstop.
const START_WAIT: Duration = Duration::from_secs(120);

/// How long one call into a ready adapter (send, decide, interrupt) may take.
/// Each writes one line to the backend; one still waiting after this has a
/// backend that stopped reading.
const ADAPTER_CALL: Duration = Duration::from_secs(30);

/// How many events a replay sends before it looks again for whether the
/// subscription is still wanted.
const REPLAY_PAGE: usize = 500;

/// How long `session.send` waits for its message to be projected, to report
/// the turn it started.
const SEND_SETTLE: Duration = Duration::from_secs(10);

/// How long reading a session's record may take before the session opens
/// with an error saying so. Records are local files, normally read in well
/// under a second; a file system that hangs (a network drive, a synced
/// folder) must not hold the session's other operations for ever.
const RECORD_READ: Duration = Duration::from_secs(30);

/// How long reading a record's title and creation time may take, and how
/// long startup waits for all of them together.
const SUMMARY_READ: Duration = Duration::from_secs(5);
const STARTUP_SUMMARIES: Duration = Duration::from_secs(10);

/// After a stop interrupts a start, how long the session task waits for the
/// adapter to report the half-started process's exit.
const CANCELLED_START_GRACE: Duration = Duration::from_secs(10);

/// Signals a connection to close; see `ConnectionState::kick` in `api`.
pub type Kick = Arc<watch::Sender<bool>>;

/// Why a session operation failed.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("no session {0}")]
    NotFound(String),
    #[error("{0}")]
    InvalidState(String),
    #[error("{0}")]
    BackendMissing(String),
    #[error(transparent)]
    Store(#[from] persistence::Error),
    #[error("{0}")]
    Adapter(AdapterError),
    #[error("{0}")]
    Internal(String),
}

/// Every session Core knows, and the adapters it can start them with.
pub struct SessionManager {
    store: Store,
    adapters: HashMap<String, Arc<dyn Adapter>>,
    /// Where Core writes each backend process's lifecycle facts.
    app_log: Arc<AppLog>,
    /// Every session in the database, by id. A std mutex: only held to look
    /// up, insert, or remove, never across an await.
    sessions: Mutex<HashMap<String, Arc<Handle>>>,
    /// Where `session.tile` notifications go, for every attached client.
    tiles: Arc<TileHub>,
    /// Set by [`SessionManager::stop_all`]; no session starts after it.
    /// Read and written under the `sessions` lock.
    closing: AtomicBool,
}

/// One session, live or not.
struct Handle {
    id: String,
    backend: String,
    /// The session's row as last written.
    row: Mutex<SessionRow>,
    /// What the session's record says of it, as last read.
    presented: Mutex<Presented>,
    /// Serialises the operations that change whether the session is live or
    /// which view it has: stop, remove, and opening a view. Held across
    /// awaits, so an async mutex.
    op: tokio::sync::Mutex<()>,
    hub: Mutex<Hub>,
    live: Mutex<Option<Arc<Live>>>,
    /// The lines exchanged with the backend since Core started it, for the
    /// developer pane.
    raw: Mutex<RawLog>,
    /// The fact that ended the backend the last time Core ran it, and when
    /// Core learnt it: shown again at the end of a view opened afresh, since
    /// it is Core's knowledge and never in the record.
    ending: Mutex<Option<(LifecycleFact, String)>>,
}

/// The session's open view, and who receives its events.
#[derive(Default)]
struct Hub {
    /// `None` until the session is first opened in this Core.
    view: Option<View>,
    /// Every event of the view, as notification lines: `events[seq]`.
    events: Vec<Arc<Published>>,
    subscribers: HashMap<u64, Subscriber>,
    next_subscriber: u64,
}

/// A session's title and when it began, which are its record's to say
/// (ADR 0017), not Ersa's to store.
#[derive(Debug, Clone)]
struct Presented {
    title: Option<String>,
    /// RFC 3339: from the record when it has said, else when Core first
    /// started the session's backend, else [`UNKNOWN_TIME`].
    created_at: String,
}

/// The creation time of a session nothing remembers starting: one from
/// before the app log, whose backend never wrote a record. It sorts first.
const UNKNOWN_TIME: &str = "1970-01-01T00:00:00.000Z";

impl Hub {
    fn last_seq(&self) -> Option<u64> {
        self.view.as_ref().and_then(View::last_seq)
    }
}

struct Subscriber {
    events: mpsc::Sender<Arc<Published>>,
    /// Set when the hub dropped this subscriber for falling behind.
    overflowed: Arc<AtomicBool>,
    /// Closes the subscriber's connection. Raised by the hub itself when it
    /// drops the subscriber: the forwarding task may be parked waiting on a
    /// client that stopped reading, and would never notice.
    kick: Kick,
}

/// One event as subscribers receive it: already a notification line.
struct Published {
    seq: u64,
    line: String,
}

/// What a running session shares between its tasks and the requests that
/// drive it.
struct Live {
    sink: Arc<CoreSink>,
    started: watch::Receiver<Started>,
    /// Raised by a stop that arrives while the backend is still starting.
    cancel: watch::Sender<bool>,
    /// The pipeline's progress, for `session.send` to wait on.
    progress: watch::Receiver<Progress>,
    /// Becomes true once the session has ended and its last line is projected.
    finished: watch::Receiver<bool>,
}

/// Where the adapter's start is.
#[derive(Clone)]
enum Started {
    Pending,
    Ready(Arc<dyn AdapterSession>),
    /// It failed, or the session has ended since.
    Gone,
}

#[derive(Debug, Clone, Copy, Default)]
struct Progress {
    /// Raw lines projected so far, which is also the index of the next one.
    lines: u64,
    turn: u32,
}

impl Live {
    /// The adapter's session, once it is ready.
    fn ready(&self) -> Option<Arc<dyn AdapterSession>> {
        match &*self.started.borrow() {
            Started::Ready(session) => Some(Arc::clone(session)),
            _ => None,
        }
    }

    /// Waits until the backend is ready, for a request that needs it.
    async fn wait_ready(&self) -> Result<Arc<dyn AdapterSession>, SessionError> {
        let mut started = self.started.clone();
        let ready = tokio::time::timeout(
            START_WAIT,
            started.wait_for(|s| !matches!(s, Started::Pending)),
        )
        .await;
        match ready {
            Ok(Ok(started)) => match &*started {
                Started::Ready(session) => Ok(Arc::clone(session)),
                _ => Err(not_running()),
            },
            Ok(Err(_)) => Err(not_running()),
            Err(_) => Err(SessionError::InvalidState(
                "the session is still starting; try again shortly".into(),
            )),
        }
    }
}

fn not_running() -> SessionError {
    SessionError::InvalidState("the session is not running".into())
}

impl Handle {
    /// A session first known to have begun at `created_at`, until its
    /// record says.
    fn new(row: SessionRow, created_at: String) -> Self {
        Handle {
            id: row.id.clone(),
            backend: row.backend.clone(),
            row: Mutex::new(row),
            presented: Mutex::new(Presented {
                title: None,
                created_at,
            }),
            op: tokio::sync::Mutex::new(()),
            hub: Mutex::default(),
            live: Mutex::new(None),
            raw: Mutex::default(),
            ending: Mutex::new(None),
        }
    }

    fn live(&self) -> Option<Arc<Live>> {
        lock(&self.live).clone()
    }

    /// The session as `session.list` reports it. A session no view has
    /// been opened for is not running in this Core, so it has ended.
    fn describe(&self) -> Session {
        let row = lock(&self.row).clone();
        let presented = lock(&self.presented).clone();
        let ending = lock(&self.ending).clone();
        let hub = lock(&self.hub);
        let (state, tile) = match &hub.view {
            Some(view) => (view.state(), view.tile().clone()),
            // Not opened yet in this Core: as its ending left it, which a
            // view would end with too (an exit with an error is `error`).
            None => {
                let (mut view, _) = View::open(&self.id, &self.backend, None, Vec::new());
                if let Some((fact, ts)) = &ending {
                    view.lifecycle(fact, ts);
                }
                (view.state(), view.tile().clone())
            }
        };
        Session {
            id: row.id,
            workspace_id: row.workspace_id,
            host_id: row.host_id,
            backend: row.backend,
            mode: row.mode,
            title: presented.title,
            state,
            created_at: presented.created_at,
            last_event_seq: hub.last_seq(),
            tile,
        }
    }
}

impl SessionManager {
    pub fn new(store: Store, adapters: Vec<Arc<dyn Adapter>>, app_log: Arc<AppLog>) -> Self {
        SessionManager {
            store,
            adapters: adapters
                .into_iter()
                .map(|adapter| (adapter.backend().to_string(), adapter))
                .collect(),
            app_log,
            sessions: Mutex::new(HashMap::new()),
            tiles: Arc::default(),
            closing: AtomicBool::new(false),
        }
    }

    /// Loads every session at startup, with how each last ended, from
    /// `last_facts` (each session's last fact in the app log), and ends those
    /// a previous Core left running.
    ///
    /// A session whose last fact is `process_started` was running when that
    /// Core crashed or was killed. On Windows its process tree died with that
    /// Core's job handle; elsewhere the CLI sees its stdin close and exits,
    /// though anything it started may linger. It gets a `core_restarted` fact
    /// in the app log, which ends its views.
    ///
    /// Each session's title and creation time are read from its record, on
    /// the host `transport_for` reaches.
    pub async fn recover(
        &self,
        facts: &HashMap<String, Facts>,
        transport_for: impl Fn(&str) -> Option<Arc<dyn Transport>>,
    ) -> persistence::Result<()> {
        let mut summaries = tokio::task::JoinSet::new();
        for row in self.store.sessions().await? {
            let known = facts.get(&row.id);
            let started = known.map_or(UNKNOWN_TIME.to_string(), |f| f.first_ts.clone());
            let handle = Arc::new(Handle::new(row, started));
            match known.map(|f| &f.last) {
                Some(last) if !last.fact.ends() => {
                    let ts = now_rfc3339();
                    let fact = LifecycleFact::CoreRestarted;
                    self.app_log.write(&handle.id, &ts, &fact);
                    *lock(&handle.ending) = Some((fact, ts));
                }
                Some(last) => *lock(&handle.ending) = Some((last.fact.clone(), last.ts.clone())),
                None => {}
            }
            if let Some(adapter) = self.adapters.get(&handle.backend) {
                let row = lock(&handle.row).clone();
                if let (Some(backend_id), Some(transport)) =
                    (row.backend_session_id, transport_for(&row.host_id))
                {
                    let (handle, adapter) = (Arc::clone(&handle), Arc::clone(adapter));
                    summaries.spawn(async move {
                        apply_summary(&handle, adapter.as_ref(), transport, &row.cwd, &backend_id)
                            .await;
                    });
                }
            }
            lock(&self.sessions).insert(handle.id.clone(), handle);
        }
        // Read side by side, and not waited on past a bound: a file system
        // that hangs leaves those titles to the next time a session opens,
        // rather than keeping Core from serving at all.
        let all = async { while summaries.join_next().await.is_some() {} };
        if tokio::time::timeout(STARTUP_SUMMARIES, all).await.is_err() {
            tracing::warn!("reading the sessions' titles took too long; some are left to later");
        }
        Ok(())
    }

    /// Whether an adapter for `backend` is registered.
    pub fn has_backend(&self, backend: &str) -> bool {
        self.adapters.contains_key(backend)
    }

    /// Receives `session.tile` notifications from now on, the latest per
    /// session.
    pub fn tile_feed(&self) -> TileFeed {
        self.tiles.feed()
    }

    fn handle(&self, id: &str) -> Result<Arc<Handle>, SessionError> {
        lock(&self.sessions)
            .get(id)
            .cloned()
            .ok_or_else(|| SessionError::NotFound(id.to_string()))
    }

    fn live(&self, id: &str) -> Result<Arc<Live>, SessionError> {
        self.handle(id)?.live().ok_or_else(not_running)
    }

    /// Whether the session's backend is running (or starting).
    pub fn is_live(&self, id: &str) -> Result<bool, SessionError> {
        Ok(self.handle(id)?.live().is_some())
    }

    /// Every session, oldest first.
    pub fn list(&self) -> Vec<Session> {
        let handles: Vec<Arc<Handle>> = lock(&self.sessions).values().cloned().collect();
        let mut sessions: Vec<Session> = handles.iter().map(|h| h.describe()).collect();
        sessions.sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
        sessions
    }

    /// One session.
    pub fn get(&self, id: &str) -> Result<Session, SessionError> {
        Ok(self.handle(id)?.describe())
    }

    /// How many sessions are running, and how many are waiting on the user.
    pub fn counts(&self) -> (u64, u64) {
        let handles: Vec<Arc<Handle>> = lock(&self.sessions).values().cloned().collect();
        let (mut running, mut waiting) = (0, 0);
        for handle in handles.iter().filter(|h| h.live().is_some()) {
            match handle.describe().state {
                LifecycleState::WaitingUser => waiting += 1,
                LifecycleState::Starting | LifecycleState::Running | LifecycleState::Idle => {
                    running += 1
                }
                _ => {}
            }
        }
        (running, waiting)
    }

    /// Records `row` and starts its backend in the background. Returns the
    /// session (state `starting`) once the row exists; the start itself
    /// shows up as events.
    pub async fn create(
        &self,
        row: SessionRow,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        backend_version: String,
    ) -> Result<Session, SessionError> {
        let adapter = self.adapters.get(&row.backend).cloned().ok_or_else(|| {
            SessionError::BackendMissing(format!("no adapter for backend {}", row.backend))
        })?;
        self.create_with(row, None, adapter, transport, config, backend_version)
            .await
    }

    /// [`SessionManager::create`] with a given adapter rather than the
    /// registered one for the session's backend, and a title to show until
    /// the record gives one: the replay server's, which feeds a recording in
    /// place of a backend.
    pub async fn create_with(
        &self,
        row: SessionRow,
        title: Option<String>,
        adapter: Arc<dyn Adapter>,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        backend_version: String,
    ) -> Result<Session, SessionError> {
        let shutting_down =
            || SessionError::InvalidState("Core is shutting down; no session can start".into());
        if self.closing.load(Ordering::SeqCst) {
            return Err(shutting_down());
        }
        self.store.insert_session(row.clone()).await?;
        let handle = Arc::new(Handle::new(row, now_rfc3339()));
        lock(&handle.presented).title = title;
        // A new session has no record yet: the backend has not been told
        // anything, so its events are the live stream's from the start.
        let (view, events) = View::open(
            &handle.id,
            &handle.backend,
            Some(adapter.projector()),
            Vec::new(),
        );
        install_view(&handle, view, events);
        {
            // Registered and launched under the lock `stop_all` takes, so a
            // session either starts before shutdown and is stopped by it, or
            // does not start at all.
            let mut sessions = lock(&self.sessions);
            if !self.closing.load(Ordering::SeqCst) {
                sessions.insert(handle.id.clone(), Arc::clone(&handle));
                self.launch(
                    Arc::clone(&handle),
                    adapter,
                    transport,
                    config,
                    backend_version,
                );
                return Ok(handle.describe());
            }
        }
        self.store.delete_session(handle.id.clone()).await?;
        Err(shutting_down())
    }

    /// Starts the session's tasks; its view must be open. The handle is live
    /// when this returns.
    fn launch(
        &self,
        handle: Arc<Handle>,
        adapter: Arc<dyn Adapter>,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        backend_version: String,
    ) {
        let (fed_tx, fed_rx) = mpsc::unbounded_channel();
        let sink = Arc::new(CoreSink::new(&handle.id, Arc::clone(&self.app_log), fed_tx));
        let (started_tx, started_rx) = watch::channel(Started::Pending);
        let (progress_tx, progress_rx) = watch::channel(Progress::default());
        let (finished_tx, finished_rx) = watch::channel(false);
        let live = Arc::new(Live {
            sink: Arc::clone(&sink),
            started: started_rx,
            cancel: watch::Sender::new(false),
            progress: progress_rx,
            finished: finished_rx,
        });
        *lock(&handle.live) = Some(Arc::clone(&live));

        let pipeline = tokio::spawn(run_pipeline(Pipeline {
            store: self.store.clone(),
            handle: Arc::clone(&handle),
            fed: fed_rx,
            progress: progress_tx,
            tiles: self.tiles.clone(),
        }));
        tokio::spawn(run_session(SessionTask {
            handle,
            live,
            adapter,
            transport,
            config,
            backend_version,
            started: started_tx,
            finished: finished_tx,
            pipeline,
        }));
    }

    /// Starts stopping a session without waiting, if it is live: what
    /// `session.stop` does the moment it is read, ahead of the requests
    /// queued before it.
    pub fn request_stop(&self, id: &str) {
        if let Ok(handle) = self.handle(id)
            && let Some(live) = handle.live()
        {
            begin_stop(&live);
        }
    }

    /// Stops a session and waits until its backend has ended and every line
    /// is projected. Stopping a session that is not running does nothing.
    pub async fn stop(&self, id: &str) -> Result<(), SessionError> {
        let handle = self.handle(id)?;
        let _op = handle.op.lock().await;
        let Some(live) = handle.live() else {
            return Ok(());
        };
        begin_stop(&live);
        wait_finished(&live, STOP_TIMEOUT).await
    }

    /// Stops every running session, for `core.shutdown`: all at once, then
    /// waits for them together.
    pub async fn stop_all(&self) {
        let running: Vec<Arc<Live>> = {
            let sessions = lock(&self.sessions);
            self.closing.store(true, Ordering::SeqCst);
            sessions
                .values()
                .filter_map(|handle| handle.live())
                .collect()
        };
        for live in &running {
            begin_stop(live);
        }
        let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
        for live in &running {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if let Err(e) = wait_finished(live, left).await {
                tracing::warn!(session = %live.sink.session_id, error = %e, "session did not stop at shutdown");
            }
        }
    }

    /// Forgets a stopped session. Its record is the backend's, and stays.
    pub async fn remove(&self, id: &str) -> Result<Session, SessionError> {
        let handle = self.handle(id)?;
        let _op = handle.op.lock().await;
        if handle.live().is_some() {
            return Err(SessionError::InvalidState(format!(
                "session {id} is running; stop it first"
            )));
        }
        let session = handle.describe();
        self.store.delete_session(id.to_string()).await?;
        lock(&self.sessions).remove(id);
        // A tile still waiting to go out would show the session again.
        self.tiles.forget(id);
        // Dropping the senders ends every subscription's forwarding task.
        // The connections stay open: the client that removed the session
        // may be one of them, and the session is gone from `session.list`.
        lock(&handle.hub).subscribers.clear();
        Ok(session)
    }

    /// Takes effect on the session's next start.
    pub async fn set_mode(&self, id: &str, mode: String) -> Result<Session, SessionError> {
        let handle = self.handle(id)?;
        self.store
            .set_session_mode(id.to_string(), mode.clone())
            .await?;
        lock(&handle.row).mode = Some(mode);
        Ok(handle.describe())
    }

    /// Sends a user message and returns the turn it belongs to.
    pub async fn send(&self, id: &str, text: &str) -> Result<u32, SessionError> {
        let live = self.live(id)?;
        let session = live.wait_ready().await?;
        let text = text.to_string();
        bounded(async move { session.send_user(&text).await }).await?;
        // The adapter recorded the message before writing it, so the sink
        // has it. Wait for the pipeline to project it: the turn it starts
        // (or joins) is known then.
        let target = live.sink.next_index();
        let mut progress = live.progress.clone();
        let settled = matches!(
            tokio::time::timeout(SEND_SETTLE, progress.wait_for(|p| p.lines >= target)).await,
            Ok(Ok(_))
        );
        if !settled {
            // Sent, but not projected in time: the turn is not known, and a
            // made-up number would hide that the pipeline is falling behind.
            return Err(SessionError::Internal(
                "the message was sent, but Core could not project it in time".into(),
            ));
        }
        let turn = progress.borrow().turn;
        Ok(turn.max(1))
    }

    pub async fn interrupt(&self, id: &str) -> Result<(), SessionError> {
        let live = self.live(id)?;
        let session = live.wait_ready().await?;
        bounded(async move { session.interrupt().await }).await
    }

    /// Answers a pending permission request (PROTO-CORE-API-R04). The answer
    /// is a host line, so `permission.decided` is projected from it.
    pub async fn decide_permission(
        &self,
        id: &str,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), SessionError> {
        let live = self.live(id)?;
        let session = live.wait_ready().await?;
        let request_id = request_id.to_string();
        bounded(async move { session.decide_permission(&request_id, decision).await }).await
    }

    /// Raw lines `from..=to` of the session's in-memory raw log, at most
    /// `limit` of them and about `max_bytes` of text.
    pub fn raw_log(
        &self,
        id: &str,
        from: u64,
        to: u64,
        limit: usize,
        max_bytes: usize,
    ) -> Result<Vec<RawLine>, SessionError> {
        let handle = self.handle(id)?;
        let raw = lock(&handle.raw);
        Ok(raw.read(from, to, limit, max_bytes))
    }

    /// Subscribes a connection to a session's events from `from_seq` on
    /// (PROTO-CORE-API-R02), opening the session's view first when it needs
    /// one (see the module's notes). `transport_for` reaches a host, to read
    /// the session record there. Returns the subscription, which stops when
    /// dropped, and the seq of the last event so far.
    ///
    /// Nothing is sent until [`Subscription::start`], so the caller can put
    /// its response on the connection first.
    pub async fn subscribe(
        &self,
        id: &str,
        from_seq: u64,
        transport_for: impl Fn(&str) -> Option<Arc<dyn Transport>>,
        outbound: mpsc::Sender<String>,
        kick: Kick,
    ) -> Result<(Subscription, Option<u64>), SessionError> {
        let handle = self.handle(id)?;
        let wants_view =
            |hub: &Hub| hub.view.is_none() || (from_seq == 0 && hub.subscribers.is_empty());
        if handle.live().is_none() && wants_view(&lock(&handle.hub)) {
            let _op = handle.op.lock().await;
            if handle.live().is_none() && wants_view(&lock(&handle.hub)) {
                let (view, events) = self.open_view(&handle, &transport_for).await?;
                // Checked again: a client may have subscribed to the view
                // that was open while the record was read, and must keep it.
                if install_view_if(&handle, view, events, wants_view) {
                    send_tile(&self.tiles, &handle.id, &handle.describe().tile);
                }
            }
        }

        let (events_tx, events_rx) = mpsc::channel(SUBSCRIBER_QUEUE);
        let overflowed = Arc::new(AtomicBool::new(false));
        // Registered and the backlog taken under one lock, the lock the
        // pipeline publishes under: every event up to `last_seq` is in the
        // backlog, and every later one will arrive on the queue. That split
        // is what makes the stream gap-free and duplicate-free.
        let (subscriber, last_seq, backlog) = {
            let mut hub = lock(&handle.hub);
            let subscriber = hub.next_subscriber;
            hub.next_subscriber += 1;
            hub.subscribers.insert(
                subscriber,
                Subscriber {
                    events: events_tx,
                    overflowed: Arc::clone(&overflowed),
                    kick: Arc::clone(&kick),
                },
            );
            let from = usize::try_from(from_seq).unwrap_or(usize::MAX);
            let backlog: Vec<Arc<Published>> = hub.events.get(from..).unwrap_or_default().to_vec();
            (subscriber, hub.last_seq(), backlog)
        };
        let (go_tx, go_rx) = oneshot::channel();
        let task = tokio::spawn(forward(Forward {
            from_seq,
            backlog,
            live: events_rx,
            outbound,
            overflowed,
            go: go_rx,
        }));
        let subscription = Subscription {
            handle: Arc::downgrade(&handle),
            subscriber,
            task: task.abort_handle(),
            go: Some(go_tx),
        };
        Ok((subscription, last_seq))
    }

    /// A fresh view of a session this Core is not running: the events of its
    /// record, then how Core last saw its backend end, if it did.
    async fn open_view(
        &self,
        handle: &Handle,
        transport_for: &impl Fn(&str) -> Option<Arc<dyn Transport>>,
    ) -> Result<(View, Vec<Envelope>), SessionError> {
        let row = lock(&handle.row).clone();
        let Some(adapter) = self.adapters.get(&row.backend) else {
            return Err(SessionError::InvalidState(format!(
                "the events of session {} cannot be read: Core has no adapter for backend {}",
                row.id, row.backend
            )));
        };
        let record = match &row.backend_session_id {
            // The backend never announced the session, so it has no record.
            None => Vec::new(),
            Some(backend_id) => {
                let read = match transport_for(&row.host_id) {
                    Some(transport) => {
                        let read = adapter.read_record(transport, &row.cwd, backend_id);
                        match tokio::time::timeout(RECORD_READ, read).await {
                            Ok(read) => read.map_err(|e| e.to_string()),
                            Err(_) => Err(format!("it took longer than {RECORD_READ:?}")),
                        }
                    }
                    None => Err(format!("host {} cannot be reached", row.host_id)),
                };
                // A record that cannot be read is something the user should
                // see, in the session, rather than a refused subscription.
                read.unwrap_or_else(|e| {
                    tracing::warn!(session = %row.id, error = %e, "could not read the session record");
                    vec![RecordEvent {
                        message_id: None,
                        ts: Some(now_rfc3339()),
                        backend_version: None,
                        event: Event::Error(ErrorEvent {
                            code: ErrorCode::Transport,
                            message: format!("the session's history could not be read: {e}"),
                            recoverable: true,
                            action: None,
                        }),
                    }]
                })
            }
        };
        // Turns taken elsewhere may have changed its title too.
        read_presented(handle, adapter.as_ref(), transport_for).await;
        let (mut view, mut events) = View::open(&row.id, &row.backend, None, record);
        if let Some((fact, ts)) = lock(&handle.ending).clone() {
            events.extend(view.lifecycle(&fact, &ts));
        }
        Ok((view, events))
    }
}

/// Reads the session's title and creation time from its record, keeping
/// what was known when the record has none or cannot be read.
async fn read_presented(
    handle: &Handle,
    adapter: &dyn Adapter,
    transport_for: &impl Fn(&str) -> Option<Arc<dyn Transport>>,
) {
    let row = lock(&handle.row).clone();
    let (Some(backend_id), Some(transport)) =
        (&row.backend_session_id, transport_for(&row.host_id))
    else {
        return;
    };
    apply_summary(handle, adapter, transport, &row.cwd, backend_id).await;
}

async fn apply_summary(
    handle: &Handle,
    adapter: &dyn Adapter,
    transport: Arc<dyn Transport>,
    cwd: &str,
    backend_id: &str,
) {
    let read = adapter.summarize_record(transport, cwd, backend_id);
    match tokio::time::timeout(SUMMARY_READ, read).await {
        Ok(Ok(Some(summary))) => {
            let mut presented = lock(&handle.presented);
            if summary.title.is_some() {
                presented.title = summary.title;
            }
            // The record's wrapper is the backend's and changes between
            // versions: a time that is not RFC 3339 is not used.
            match summary.created_at {
                Some(at) if is_rfc3339(&at) => presented.created_at = at,
                Some(at) => {
                    tracing::warn!(session = %handle.id, created_at = %at, "the session record's first time is not RFC 3339; not used");
                }
                None => {}
            }
        }
        Ok(Ok(None)) => {}
        Ok(Err(e)) => {
            tracing::warn!(session = %handle.id, error = %e, "could not read the session record's title");
        }
        Err(_) => {
            tracing::warn!(session = %handle.id, "reading the session record's title took too long");
        }
    }
}

/// Whether `text` has the shape of an RFC 3339 time:
/// `YYYY-MM-DDTHH:MM:SS`, an optional fraction, then `Z` or an offset.
fn is_rfc3339(text: &str) -> bool {
    let bytes = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|d| d.iter().all(u8::is_ascii_digit))
    };
    let at = |i: usize, c: u8| bytes.get(i) == Some(&c);
    if !(digits(0..4)
        && at(4, b'-')
        && digits(5..7)
        && at(7, b'-')
        && digits(8..10)
        && (at(10, b'T') || at(10, b't'))
        && digits(11..13)
        && at(13, b':')
        && digits(14..16)
        && at(16, b':')
        && digits(17..19))
    {
        return false;
    }
    let mut rest = &text[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        let end = fraction
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(fraction.len());
        if end == 0 {
            return false;
        }
        rest = &fraction[end..];
    }
    match rest.as_bytes() {
        [b'Z' | b'z'] => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] => [h1, h2, m1, m2].iter().all(|d| d.is_ascii_digit()),
        _ => false,
    }
}

/// Makes `view`, whose events so far are `events`, the session's open view.
/// Subscribers of the view it replaces had none: a view is replaced only
/// while nobody reads it.
fn install_view(handle: &Handle, view: View, events: Vec<Envelope>) {
    install_view_if(handle, view, events, |_| true);
}

/// [`install_view`] when `wanted` still says so, checked under the lock
/// that installs it: a client that subscribed to the open view meanwhile
/// keeps it. Returns whether it was installed.
fn install_view_if(
    handle: &Handle,
    view: View,
    events: Vec<Envelope>,
    wanted: impl Fn(&Hub) -> bool,
) -> bool {
    let published = events
        .iter()
        .map(|event| Arc::new(published(&handle.id, event)))
        .collect();
    let mut hub = lock(&handle.hub);
    if !wanted(&hub) {
        return false;
    }
    hub.view = Some(view);
    hub.events = published;
    true
}

/// Runs one adapter call, answering after at most [`ADAPTER_CALL`]: a
/// backend that stopped reading its stdin would otherwise hold the write,
/// and with it every later request on the client's connection, until
/// someone stopped the session from elsewhere.
///
/// The call runs in a task of its own and is never cancelled: dropping a
/// write halfway would leave half a line in the backend's stdin, which the
/// next line would be glued onto. A call still stuck when the request gives
/// up finishes, or fails, when the session is stopped.
async fn bounded(
    call: impl std::future::Future<Output = Result<(), AdapterError>> + Send + 'static,
) -> Result<(), SessionError> {
    match tokio::time::timeout(ADAPTER_CALL, tokio::spawn(call)).await {
        Ok(Ok(result)) => result.map_err(SessionError::Adapter),
        Ok(Err(e)) => Err(SessionError::Internal(format!("the adapter failed: {e}"))),
        Err(_) => Err(SessionError::Adapter(AdapterError::Io(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("the backend did not take the line within {ADAPTER_CALL:?}"),
        )))),
    }
}

/// Asks a live session to stop without waiting: marks the stop as the
/// user's, interrupts a start in progress, and stops a ready backend.
fn begin_stop(live: &Arc<Live>) {
    live.sink.stop_requested.store(true, Ordering::SeqCst);
    live.cancel.send_replace(true);
    if let Some(session) = live.ready() {
        let id = live.sink.session_id.clone();
        tokio::spawn(async move {
            if let Err(e) = session.stop().await {
                tracing::warn!(session = %id, error = %e, "stopping the backend failed");
            }
        });
    }
}

async fn wait_finished(live: &Live, timeout: Duration) -> Result<(), SessionError> {
    let mut finished = live.finished.clone();
    match tokio::time::timeout(timeout, finished.wait_for(|done| *done)).await {
        // `Err` inside: the session task is gone, which means it finished.
        Ok(_) => Ok(()),
        Err(_) => Err(SessionError::Internal(format!(
            "the backend did not stop within {timeout:?}"
        ))),
    }
}

// -- The sink ------------------------------------------------------------------

/// What the sink queues for the pipeline, in the order it happened.
enum Fed {
    /// A line that crossed the backend's stdio.
    Line(RawLine),
    /// One of Core's lifecycle facts, learnt at `ts`.
    Fact { fact: LifecycleFact, ts: String },
}

/// Core's side of `AdapterSink` for one live session.
struct CoreSink {
    session_id: String,
    /// Where each lifecycle fact is written as the sink takes it in, before
    /// anything it describes can happen.
    app_log: Arc<AppLog>,
    inner: Mutex<SinkInner>,
    /// Set by `session.stop`: the exit that follows is the user's doing.
    stop_requested: AtomicBool,
    /// Becomes true when the adapter reports the process exit, even one held
    /// back because the start had not finished.
    exit_seen: watch::Sender<bool>,
    /// Becomes true when the fact that ends the session is queued.
    ended: watch::Sender<bool>,
}

struct SinkInner {
    next_index: u64,
    /// `None` once Core has closed the sink, which ends the pipeline.
    fed: Option<mpsc::UnboundedSender<Fed>>,
    start_done: bool,
    /// An exit reported before the start finished, recorded when it does:
    /// the fact that ends the session must come last.
    deferred_exit: Option<io::Result<ProcessExit>>,
    /// The ending fact is in; nothing more from the adapter is recorded.
    ended: bool,
}

/// Why a start did not produce a running session.
struct StartFailure {
    message: String,
    stopped_by_user: bool,
}

impl CoreSink {
    fn new(session_id: &str, app_log: Arc<AppLog>, fed: mpsc::UnboundedSender<Fed>) -> Self {
        CoreSink {
            session_id: session_id.to_string(),
            app_log,
            inner: Mutex::new(SinkInner {
                next_index: 0,
                fed: Some(fed),
                start_done: false,
                deferred_exit: None,
                ended: false,
            }),
            stop_requested: AtomicBool::new(false),
            exit_seen: watch::Sender::new(false),
            ended: watch::Sender::new(false),
        }
    }

    /// Queues `item`. Called with the lock held, so items follow the order
    /// of the calls. Unbounded, so this never blocks the adapter's read
    /// loop; the pipeline only projects, which keeps up with any backend.
    fn queue(inner: &mut SinkInner, item: Fed) {
        if let Some(fed) = &inner.fed {
            let _ = fed.send(item);
        }
    }

    /// Stamps and queues one line.
    fn push_line(inner: &mut SinkInner, dir: Direction, line: String) {
        let raw = RawLine {
            index: inner.next_index,
            ts: now_rfc3339(),
            dir,
            line,
        };
        inner.next_index += 1;
        Self::queue(inner, Fed::Line(raw));
    }

    /// Writes a fact to the app log, then queues it. Written here rather
    /// than by the pipeline, so `process_started` is on disk before the
    /// process starts, and a crash just after still leaves the fact that
    /// startup repairs from.
    fn push_fact(&self, inner: &mut SinkInner, fact: LifecycleFact) {
        let ts = now_rfc3339();
        self.app_log.write(&self.session_id, &ts, &fact);
        Self::queue(inner, Fed::Fact { fact, ts });
    }

    fn record_fact(&self, fact: LifecycleFact) {
        self.push_fact(&mut lock(&self.inner), fact);
    }

    /// The index the next line will get.
    fn next_index(&self) -> u64 {
        lock(&self.inner).next_index
    }

    fn exit_fact(&self, exit: io::Result<ProcessExit>) -> LifecycleFact {
        let (code, error) = match exit {
            Ok(exit) => (exit.code, None),
            Err(e) => (None, Some(e.to_string())),
        };
        LifecycleFact::ProcessExited {
            code,
            stopped_by_user: self.stop_requested.load(Ordering::SeqCst),
            error,
        }
    }

    /// The session task's report of how the start went. Records the fact
    /// that ends the session when the start failed, or when the process
    /// already exited while it was starting.
    fn start_finished(&self, outcome: Result<(), StartFailure>) {
        let mut inner = lock(&self.inner);
        inner.start_done = true;
        let fact = match outcome {
            Ok(()) => match inner.deferred_exit.take() {
                Some(exit) => self.exit_fact(exit),
                None => return,
            },
            Err(failure) => LifecycleFact::StartFailed {
                message: failure.message,
                code: inner
                    .deferred_exit
                    .take()
                    .and_then(|exit| exit.ok())
                    .and_then(|exit| exit.code),
                stopped_by_user: failure.stopped_by_user,
            },
        };
        if inner.ended {
            return;
        }
        self.push_fact(&mut inner, fact);
        inner.ended = true;
        drop(inner);
        self.ended.send_replace(true);
    }

    /// Stops queueing, which ends the pipeline once it has projected what is
    /// already queued.
    fn close(&self) {
        lock(&self.inner).fed = None;
    }
}

impl AdapterSink for CoreSink {
    fn record(&self, dir: Direction, line: String) {
        let mut inner = lock(&self.inner);
        if inner.ended {
            // The adapter promised the exit comes after its last line.
            tracing::warn!(session = %self.session_id, "line recorded after the session ended; ignored");
            return;
        }
        Self::push_line(&mut inner, dir, line);
    }

    fn stderr_line(&self, line: String) {
        // Diagnostics, not protocol; trimmed so one huge line cannot flood
        // the log.
        let shown: String = line.chars().take(2000).collect();
        tracing::debug!(session = %self.session_id, stderr = %shown, "backend stderr");
    }

    fn process_exited(&self, exit: io::Result<ProcessExit>) {
        let mut inner = lock(&self.inner);
        if inner.ended {
            return;
        }
        if inner.start_done {
            let fact = self.exit_fact(exit);
            self.push_fact(&mut inner, fact);
            inner.ended = true;
            drop(inner);
            self.ended.send_replace(true);
        } else {
            inner.deferred_exit = Some(exit);
            drop(inner);
        }
        self.exit_seen.send_replace(true);
    }
}

// -- The session task ----------------------------------------------------------

struct SessionTask {
    handle: Arc<Handle>,
    live: Arc<Live>,
    adapter: Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
    config: SessionConfig,
    backend_version: String,
    started: watch::Sender<Started>,
    finished: watch::Sender<bool>,
    pipeline: tokio::task::JoinHandle<()>,
}

async fn run_session(task: SessionTask) {
    let SessionTask {
        handle,
        live,
        adapter,
        transport,
        config,
        backend_version,
        started,
        finished,
        pipeline,
    } = task;
    let sink = Arc::clone(&live.sink);
    sink.record_fact(LifecycleFact::ProcessStarted {
        backend: handle.backend.clone(),
        backend_version,
    });

    let mut cancel = live.cancel.subscribe();
    let as_sink: Arc<dyn AdapterSink> = sink.clone();
    // The start runs in a task of its own, so an adapter that panics fails
    // the start instead of leaving the session live forever.
    let (starter, start_transport) = (Arc::clone(&adapter), Arc::clone(&transport));
    let mut start =
        tokio::spawn(async move { starter.start(start_transport, config, as_sink).await });
    let outcome = tokio::select! {
        result = &mut start => Some(result.unwrap_or_else(|e| {
            Err(AdapterError::Protocol(format!("the adapter failed while starting: {e}")))
        })),
        // A stop during the start drops the start: an adapter's half-made
        // session kills its process when dropped.
        () = raised(&mut cancel) => {
            start.abort();
            None
        }
    };
    let mut pipeline = pipeline;
    let mut pipeline_done = false;
    match outcome {
        Some(Ok(session)) => {
            let session: Arc<dyn AdapterSession> = Arc::from(session);
            started.send_replace(Started::Ready(Arc::clone(&session)));
            sink.start_finished(Ok(()));
            // A stop that arrived as the start completed saw nothing to stop.
            if *cancel.borrow() {
                begin_stop(&live);
            }
            // Until the process exits, by itself or because of a stop. A
            // pipeline that died (a bug) can no longer show anything, so the
            // backend is stopped rather than left running unseen.
            let mut ended = sink.ended.subscribe();
            let pipeline_result = tokio::select! {
                _ = ended.wait_for(|ended| *ended) => None,
                result = &mut pipeline => Some(result),
            };
            if let Some(result) = pipeline_result {
                pipeline_done = true;
                tracing::error!(session = %handle.id, error = ?result.err(), "the session's pipeline ended early; stopping the backend");
                begin_stop(&live);
                let _ = tokio::time::timeout(STOP_TIMEOUT, ended.wait_for(|ended| *ended)).await;
            }
        }
        Some(Err(e)) => {
            tracing::warn!(session = %handle.id, error = %e, "the backend did not start");
            sink.start_finished(Err(StartFailure {
                message: e.to_string(),
                stopped_by_user: false,
            }));
        }
        None => {
            let mut exit_seen = sink.exit_seen.subscribe();
            let _ =
                tokio::time::timeout(CANCELLED_START_GRACE, exit_seen.wait_for(|seen| *seen)).await;
            sink.start_finished(Err(StartFailure {
                message: "stopped before the backend was ready".into(),
                stopped_by_user: true,
            }));
        }
    }

    // Our handle on the adapter's session goes; a session still held by a
    // request in flight goes when that request ends.
    started.send_replace(Started::Gone);
    sink.close();
    if !pipeline_done && let Err(e) = pipeline.await {
        tracing::error!(session = %handle.id, error = %e, "the session's pipeline failed");
    }
    if let Some(view) = lock(&handle.hub).view.as_mut() {
        view.finish_live();
    }
    // The record now holds the session's turns, and with them its title.
    let row = lock(&handle.row).clone();
    if let Some(backend_id) = &row.backend_session_id {
        apply_summary(&handle, adapter.as_ref(), transport, &row.cwd, backend_id).await;
    }
    *lock(&handle.live) = None;
    finished.send_replace(true);
}

/// Returns once the flag is raised; never if its sender is gone.
async fn raised(flag: &mut watch::Receiver<bool>) {
    if flag.wait_for(|raised| *raised).await.is_err() {
        std::future::pending::<()>().await;
    }
}

// -- The pipeline --------------------------------------------------------------

struct Pipeline {
    store: Store,
    handle: Arc<Handle>,
    fed: mpsc::UnboundedReceiver<Fed>,
    progress: watch::Sender<Progress>,
    tiles: Arc<TileHub>,
}

async fn run_pipeline(pipeline: Pipeline) {
    let Pipeline {
        store,
        handle,
        mut fed,
        progress,
        tiles,
    } = pipeline;
    let mut last_tile = handle.describe().tile;
    let mut lines_done = 0;
    // Ends once the sink is closed and everything queued has been taken.
    while let Some(first) = fed.recv().await {
        let mut batch = vec![first];
        while batch.len() < BATCH {
            match fed.try_recv() {
                Ok(item) => batch.push(item),
                Err(_) => break,
            }
        }
        // How the backend ended, to show again when the session is reopened.
        for item in &batch {
            if let Fed::Fact { fact, ts } = item
                && fact.ends()
            {
                *lock(&handle.ending) = Some((fact.clone(), ts.clone()));
            }
        }
        let (tile, turn, announced) = {
            let mut hub = lock(&handle.hub);
            let Some(view) = hub.view.as_mut() else {
                // Never: a live session's view is open from its start.
                tracing::error!(session = %handle.id, "a live session has no view; its events are lost");
                continue;
            };
            let mut events = Vec::new();
            for item in batch {
                match item {
                    Fed::Line(line) => {
                        events.extend(view.live(&line));
                        lines_done = line.index + 1;
                        lock(&handle.raw).push(line);
                    }
                    Fed::Fact { fact, ts } => events.extend(view.lifecycle(&fact, &ts)),
                }
            }
            let announced = view.backend_session_id().map(String::from);
            let (tile, turn) = (view.tile().clone(), view.turn());
            publish(&mut hub, &handle.id, &events);
            (tile, turn, announced)
        };

        // The backend's own id finds the session's record later; it is the
        // one thing about the session's history Ersa keeps.
        if let Some(backend_id) = announced {
            let known = lock(&handle.row).backend_session_id.clone();
            if known.as_deref() != Some(backend_id.as_str()) {
                match store
                    .set_backend_session_id(handle.id.clone(), backend_id.clone())
                    .await
                {
                    Ok(()) => lock(&handle.row).backend_session_id = Some(backend_id),
                    Err(e) => {
                        tracing::error!(session = %handle.id, error = %e, "could not record the backend's session id");
                    }
                }
            }
        }
        if tile != last_tile {
            send_tile(&tiles, &handle.id, &tile);
            last_tile = tile;
        }
        progress.send_replace(Progress {
            lines: lines_done,
            turn,
        });
    }
}

/// Adds new events to the view's list and offers them to every subscriber.
/// A subscriber whose queue is full is dropped, never waited for: one slow
/// client must not stall the session for everyone else.
fn publish(hub: &mut Hub, session_id: &str, events: &[Envelope]) {
    if events.is_empty() {
        return;
    }
    let published: Vec<Arc<Published>> = events
        .iter()
        .map(|event| Arc::new(published(session_id, event)))
        .collect();
    hub.events.extend(published.iter().cloned());
    hub.subscribers.retain(|_, subscriber| {
        for event in &published {
            match subscriber.events.try_send(Arc::clone(event)) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    subscriber.overflowed.store(true, Ordering::SeqCst);
                    subscriber.kick.send_replace(true);
                    return false;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => return false,
            }
        }
        true
    });
}

/// An event as a `session.event` notification line.
fn published(session_id: &str, event: &Envelope) -> Published {
    let params = SessionEventParams {
        session_id: session_id.to_string(),
        event: event.clone(),
    };
    Published {
        seq: event.seq,
        line: notification_line("session.event", &params),
    }
}

fn send_tile(tiles: &TileHub, session_id: &str, tile: &Tile) {
    let params = SessionTileParams {
        session_id: session_id.to_string(),
        tile: tile.clone(),
    };
    tiles.publish(session_id, notification_line("session.tile", &params));
}

/// Hands `session.tile` notifications to every attached connection,
/// keeping only the latest per session for each: a tile is the whole state,
/// so a connection busy with a long replay gets the newest one when it is
/// ready, never a backlog, and is never closed for it.
#[derive(Default)]
struct TileHub {
    inboxes: Mutex<Vec<Weak<TileInbox>>>,
}

#[derive(Default)]
struct TileInbox {
    /// The newest tile line per session not yet taken.
    latest: Mutex<HashMap<String, Arc<String>>>,
    ready: Notify,
}

impl TileHub {
    fn feed(&self) -> TileFeed {
        let inbox = Arc::new(TileInbox::default());
        lock(&self.inboxes).push(Arc::downgrade(&inbox));
        TileFeed { inbox }
    }

    /// Drops any tile for `session_id` not yet taken, for a removed session.
    fn forget(&self, session_id: &str) {
        for inbox in lock(&self.inboxes).iter().filter_map(Weak::upgrade) {
            lock(&inbox.latest).remove(session_id);
        }
    }

    fn publish(&self, session_id: &str, line: String) {
        let line = Arc::new(line);
        let mut inboxes = lock(&self.inboxes);
        // A connection that has gone dropped its feed; forget its inbox.
        inboxes.retain(|inbox| {
            let Some(inbox) = inbox.upgrade() else {
                return false;
            };
            lock(&inbox.latest).insert(session_id.to_string(), Arc::clone(&line));
            // Stores a permit when nobody waits, so a wake is never lost.
            inbox.ready.notify_one();
            true
        });
    }
}

/// One connection's view of tile changes.
pub struct TileFeed {
    inbox: Arc<TileInbox>,
}

impl TileFeed {
    /// The tile lines that changed since the last call, one per session;
    /// waits until there is at least one.
    pub async fn next(&mut self) -> Vec<Arc<String>> {
        loop {
            {
                let mut latest = lock(&self.inbox.latest);
                if !latest.is_empty() {
                    return latest.drain().map(|(_, line)| line).collect();
                }
            }
            self.inbox.ready.notified().await;
        }
    }
}

fn notification_line(method: &str, params: &impl serde::Serialize) -> String {
    let notification = Notification::new(method, params).expect("notification params serialize");
    serde_json::to_string(&notification).expect("notifications serialize")
}

// -- Subscriptions -------------------------------------------------------------

/// One connection's subscription to one session. Dropping it stops it.
pub struct Subscription {
    handle: Weak<Handle>,
    subscriber: u64,
    task: AbortHandle,
    go: Option<oneshot::Sender<()>>,
}

impl Subscription {
    /// Lets events flow, once the subscribe response is on the connection.
    pub fn start(&mut self) {
        if let Some(go) = self.go.take() {
            let _ = go.send(());
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.task.abort();
        if let Some(handle) = self.handle.upgrade() {
            lock(&handle.hub).subscribers.remove(&self.subscriber);
        }
    }
}

struct Forward {
    from_seq: u64,
    /// The events from `from_seq` already in the view when the subscription
    /// began; later ones arrive on `live`.
    backlog: Vec<Arc<Published>>,
    live: mpsc::Receiver<Arc<Published>>,
    outbound: mpsc::Sender<String>,
    overflowed: Arc<AtomicBool>,
    go: oneshot::Receiver<()>,
}

/// Replays, then forwards live events, to one connection. This task, not
/// the pipeline, waits for the client: `outbound.send` waits for room in
/// the connection's write queue.
async fn forward(task: Forward) {
    let Forward {
        from_seq,
        backlog,
        mut live,
        outbound,
        overflowed,
        go,
    } = task;
    // Dropped unsent: the subscribe response never went out.
    if go.await.is_err() {
        return;
    }

    let mut next = from_seq;
    for page in backlog.chunks(REPLAY_PAGE) {
        if overflowed.load(Ordering::SeqCst) {
            // Dropped meanwhile for falling behind; the hub closed the
            // connection.
            return;
        }
        for event in page {
            if outbound.send(event.line.clone()).await.is_err() {
                return;
            }
            next = event.seq + 1;
        }
    }

    loop {
        tokio::select! {
            event = live.recv() => match event {
                Some(event) if event.seq < next => {}
                Some(event) => {
                    if outbound.send(event.line.clone()).await.is_err() {
                        return;
                    }
                }
                None => break,
            },
            // The connection is gone; nothing to forward to.
            () = outbound.closed() => return,
        }
    }
    // Ended because the hub dropped this subscriber: when the client fell
    // behind, the hub has already closed its connection.
}

/// Locks a std mutex, carrying on if a panicking thread poisoned it: every
/// value behind these locks stays consistent between single updates.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::is_rfc3339;

    #[test]
    fn only_rfc3339_times_are_taken_from_a_record() {
        for good in [
            "2026-11-01T20:56:51.893Z",
            "2026-11-01T20:56:51Z",
            "2026-11-01T20:56:51+02:00",
        ] {
            assert!(is_rfc3339(good), "{good}");
        }
        for bad in [
            "",
            "yesterday",
            "2026-11-01",
            "2026-11-01T20:56:51",
            "2026-11-01T20:56:51.Z",
        ] {
            assert!(!is_rfc3339(bad), "{bad}");
        }
    }
}
