//! Session manager: the lifecycle of every session (created, starting,
//! running, waiting on user, idle, ended). Routes user input and permission
//! decisions to adapters, and raw backend output to the raw log.
//!
//! One running session is three tasks and a sink:
//!
//! - The **sink** ([`CoreSink`]) is what the adapter records lines into. It
//!   stamps each line with the next index and the time and queues it, never
//!   blocking, as `AdapterSink` requires. Stamping under one lock is what
//!   keeps the raw log in the order lines happened.
//! - The **pipeline** task takes queued lines in batches, projects them, and
//!   writes lines, events, and the session's summary row in one transaction.
//!   Only then does it hand the events to subscribers, so an event a client
//!   sees is always one it can replay later.
//! - The **session** task starts the backend through its adapter, waits for
//!   the process to end, then closes the sink, lets the pipeline drain, and
//!   marks the session no longer live.
//! - Each **subscription** has a task of its own that replays the cached
//!   events a client asked for and then forwards live ones. The pipeline
//!   never waits for a client: it offers each event to a subscription's queue
//!   and drops a subscription whose queue is full, and that subscription's
//!   connection is closed, so the client knows to reconnect and resubscribe.
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
    RawLine, SessionConfig, Transport,
};
use ersa_protocol::core_api::rpc::Notification;
use ersa_protocol::core_api::{Session, SessionEventParams, SessionTileParams, Tile};
use ersa_protocol::events::{Envelope, LifecycleState};
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::task::AbortHandle;

use crate::persistence::{self, LogBatch, SessionProgress, Store, now_rfc3339};
use crate::raw_log::{CoreRecord, Projection};

/// How many events may wait for one subscriber before it counts as unable
/// to keep up. Streaming text produces many small events, so this is
/// generous; a client further behind than this is disconnected.
const SUBSCRIBER_QUEUE: usize = 4096;

/// The most raw lines the pipeline writes in one transaction, and the most
/// bytes of them (a batch always takes at least one line).
const BATCH: usize = 256;
const BATCH_BYTES: usize = 8 * 1024 * 1024;

/// How many cached events a replay reads at a time.
const REPLAY_PAGE: u64 = 500;

/// How long `session.stop` waits for the backend to end and its last lines
/// to be stored. The Claude Code adapter kills after three seconds; this is
/// the backstop for an adapter that never returns.
const STOP_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a request waits for a session that is still starting to become
/// ready. Starting is bounded by the adapter's own handshake timeout; this
/// is the backstop.
const START_WAIT: Duration = Duration::from_secs(120);

/// How long one call into a ready adapter (send, decide, interrupt) may take.
/// Each writes one line to the backend; one still waiting after this has a
/// backend that stopped reading.
const ADAPTER_CALL: Duration = Duration::from_secs(30);

/// The most bytes of cached events a replay reads at a time (always at least
/// one event, however large).
const REPLAY_BYTES: usize = 8 * 1024 * 1024;

/// How long `session.send` waits for its message to be projected, to report
/// the turn it started.
const SEND_SETTLE: Duration = Duration::from_secs(10);

/// After a stop interrupts a start, how long the session task waits for the
/// adapter to report the half-started process's exit.
const CANCELLED_START_GRACE: Duration = Duration::from_secs(10);

/// Delays between attempts at a raw-log write that failed, about ten
/// seconds in all. A database that cannot take the write by then is not
/// coming back soon; the session is stopped rather than left running with
/// nothing recorded.
const WRITE_RETRY: [Duration; 5] = [
    Duration::from_millis(50),
    Duration::from_millis(200),
    Duration::from_secs(1),
    Duration::from_secs(3),
    Duration::from_secs(5),
];

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
    /// The adapter projector version its cached events are made with;
    /// `None` when no adapter for its backend is registered, or its cache
    /// could not be repaired. A successful re-projection sets it.
    projection_version: Mutex<Option<u32>>,
    /// Serialises the operations that change whether the session is live:
    /// stop, remove, and re-project. Held across awaits, so an async mutex.
    op: tokio::sync::Mutex<()>,
    hub: Mutex<Hub>,
    live: Mutex<Option<Arc<Live>>>,
}

/// Who receives a session's events, and how far they have been published.
#[derive(Default)]
struct Hub {
    /// The seq of the last event handed to subscribers, which is also the
    /// last one committed to the cache.
    last_seq: Option<u64>,
    subscribers: HashMap<u64, Subscriber>,
    next_subscriber: u64,
}

struct Subscriber {
    events: mpsc::Sender<Arc<Published>>,
    /// Set when the pipeline dropped this subscriber for falling behind.
    overflowed: Arc<AtomicBool>,
    /// Closes the subscriber's connection. Raised by the pipeline itself
    /// when it drops the subscriber: the forwarding task may be parked
    /// waiting on a client that stopped reading, and would never notice.
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
    /// Becomes true once the session has ended and its last line is stored.
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
    /// Raw lines stored so far, which is also the index of the next one.
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
    fn new(
        id: &str,
        backend: &str,
        projection_version: Option<u32>,
        last_seq: Option<u64>,
    ) -> Self {
        Handle {
            id: id.to_string(),
            backend: backend.to_string(),
            projection_version: Mutex::new(projection_version),
            op: tokio::sync::Mutex::new(()),
            hub: Mutex::new(Hub {
                last_seq,
                ..Hub::default()
            }),
            live: Mutex::new(None),
        }
    }

    fn live(&self) -> Option<Arc<Live>> {
        lock(&self.live).clone()
    }
}

impl SessionManager {
    pub fn new(store: Store, adapters: Vec<Arc<dyn Adapter>>) -> Self {
        SessionManager {
            store,
            adapters: adapters
                .into_iter()
                .map(|adapter| (adapter.backend().to_string(), adapter))
                .collect(),
            sessions: Mutex::new(HashMap::new()),
            tiles: Arc::default(),
            closing: AtomicBool::new(false),
        }
    }

    /// Loads every session at startup and repairs what a previous Core left.
    ///
    /// A session still marked live was running when that Core ended without
    /// recording the backend's exit (it crashed or was killed). On Windows
    /// its process tree died with that Core's job handle; elsewhere the CLI
    /// sees its stdin close and exits, though anything it started may
    /// linger. It gets a `core_restarted` record, which ends it. A cached
    /// projection made by another projector version, or one that does not
    /// reach the end of the raw log, is re-derived.
    ///
    /// A session that cannot be repaired is logged and left as it is: one
    /// bad session must not keep Core from starting for all the others.
    pub async fn recover(&self) -> persistence::Result<()> {
        for session in self.store.sessions().await? {
            let (last_seq, version) = match self.recover_one(&session).await {
                Ok(recovered) => recovered,
                Err(e) => {
                    tracing::error!(session = %session.id, error = %e, "could not repair the session; leaving it as it is");
                    // Its cache may be from another version, so its events
                    // are not served until a re-projection works, and it is
                    // not shown as still running.
                    if is_live(session.state)
                        && let Err(e) = self
                            .store
                            .set_session_state(session.id.clone(), LifecycleState::Error)
                            .await
                    {
                        tracing::error!(session = %session.id, error = %e, "could not mark the session as failed");
                    }
                    (session.last_event_seq, None)
                }
            };
            let handle = Handle::new(&session.id, &session.backend, version, last_seq);
            lock(&self.sessions).insert(session.id.clone(), Arc::new(handle));
        }
        Ok(())
    }

    /// Repairs one session as [`SessionManager::recover`] describes, and
    /// returns the seq of its last event and the projector version its cache
    /// is now made with.
    ///
    /// Whether the session ended is read from its projection, not from its
    /// row: a session whose ending was never recorded (its Core died, or its
    /// raw log could not be written) gets `core_restarted` whatever the row
    /// says.
    async fn recover_one(
        &self,
        session: &Session,
    ) -> persistence::Result<(Option<u64>, Option<u32>)> {
        let restarted = || CoreRecord::CoreRestarted.to_line();
        let Some(adapter) = self.adapters.get(&session.backend) else {
            tracing::warn!(session = %session.id, backend = %session.backend, "no adapter for the session's backend; its events stay as they are");
            if is_live(session.state) {
                // It cannot be running: end it in the log, which a Core with
                // the adapter projects, and on its row.
                self.store
                    .end_unprojected(session.id.clone(), now_rfc3339(), restarted())
                    .await?;
            }
            return Ok((session.last_event_seq, None));
        };
        let version = adapter.projector().version();
        // An unreadable cache is re-derived like a stale one.
        let current = self
            .store
            .projection_is_current(session.id.clone(), version)
            .await
            .unwrap_or(false);
        if current
            && self
                .store
                .last_event_ends_session(session.id.clone())
                .await
                .unwrap_or(false)
        {
            return Ok((session.last_event_seq, Some(version)));
        }
        tracing::info!(session = %session.id, "re-deriving the session's projection");
        let projection = Projection::new(&session.id, &session.backend, adapter.projector());
        let (mut projection, _) = self
            .store
            .rebuild_projection(session.id.clone(), projection)
            .await?;
        if !projection.ended() {
            // Carry on from the end of the log with the record that ends it.
            let line = RawLine {
                index: self.store.next_raw_index(session.id.clone()).await?,
                ts: now_rfc3339(),
                dir: Direction::Core,
                line: restarted(),
            };
            let events = projection.project(&line);
            self.store
                .append_session_log(Arc::new(LogBatch {
                    session_id: session.id.clone(),
                    lines: vec![line],
                    events,
                    projection_version: version,
                    progress: SessionProgress::of(&projection),
                }))
                .await?;
        }
        Ok((projection.last_seq(), Some(version)))
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

    /// Records `session` (state `starting`) and starts its backend in the
    /// background. Returns once the row exists; the start itself shows up as
    /// events.
    pub async fn create(
        &self,
        session: Session,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        backend_version: String,
    ) -> Result<Session, SessionError> {
        let adapter = self
            .adapters
            .get(&session.backend)
            .cloned()
            .ok_or_else(|| {
                SessionError::BackendMissing(format!("no adapter for backend {}", session.backend))
            })?;
        let shutting_down =
            || SessionError::InvalidState("Core is shutting down; no session can start".into());
        if self.closing.load(Ordering::SeqCst) {
            return Err(shutting_down());
        }
        self.store.insert_session(session.clone()).await?;
        let projection = Projection::new(&session.id, &session.backend, adapter.projector());
        let handle = Arc::new(Handle::new(
            &session.id,
            &session.backend,
            Some(projection.projector_version()),
            None,
        ));
        {
            // Registered and launched under the lock `stop_all` takes, so a
            // session either starts before shutdown and is stopped by it, or
            // does not start at all.
            let mut sessions = lock(&self.sessions);
            if !self.closing.load(Ordering::SeqCst) {
                sessions.insert(session.id.clone(), Arc::clone(&handle));
                self.launch(
                    handle,
                    adapter,
                    transport,
                    config,
                    backend_version,
                    projection,
                );
                return Ok(session);
            }
        }
        self.store.delete_session(session.id.clone()).await?;
        Err(shutting_down())
    }

    /// Starts the session's tasks. The handle is live when this returns.
    fn launch(
        &self,
        handle: Arc<Handle>,
        adapter: Arc<dyn Adapter>,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        backend_version: String,
        projection: Projection,
    ) {
        let (lines_tx, lines_rx) = mpsc::unbounded_channel();
        let sink = Arc::new(CoreSink::new(&handle.id, lines_tx));
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
            projection,
            lines: lines_rx,
            progress: progress_tx,
            tiles: self.tiles.clone(),
        }));
        tokio::spawn(run_session(SessionTask {
            store: self.store.clone(),
            tiles: Arc::clone(&self.tiles),
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
    /// is stored. Stopping a session that is not running does nothing.
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

    /// Deletes a stopped session with its raw log and events.
    pub async fn remove(&self, id: &str) -> Result<Session, SessionError> {
        let handle = self.handle(id)?;
        let _op = handle.op.lock().await;
        if handle.live().is_some() {
            return Err(SessionError::InvalidState(format!(
                "session {id} is running; stop it first"
            )));
        }
        let session = self
            .store
            .session(id.to_string())
            .await?
            .ok_or_else(|| SessionError::NotFound(id.to_string()))?;
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
            // Sent, but not stored in time: the turn is not known, and a
            // made-up number would hide that the raw log is falling behind.
            return Err(SessionError::Internal(
                "the message was sent, but Core could not store it in time".into(),
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

    /// Discards the cached projection and re-derives it from the raw log.
    /// Returns whether any event changed.
    ///
    /// When one did, subscribers were sent events that no longer hold, so
    /// their connections are closed and their clients resubscribe; the
    /// caller's own connection (`caller`) is left open, and its subscription
    /// to this session simply ends, since it asked for the change and gets
    /// the answer. Same-version re-projection changes nothing and disturbs
    /// nobody.
    pub async fn reproject(&self, id: &str, caller: &Kick) -> Result<bool, SessionError> {
        let handle = self.handle(id)?;
        let _op = handle.op.lock().await;
        if handle.live().is_some() {
            // A live session is always projected by this build's projector,
            // so there is nothing to re-derive until it stops.
            return Err(SessionError::InvalidState(format!(
                "session {id} is running; it is already projected with the current version"
            )));
        }
        let adapter = self.adapters.get(&handle.backend).ok_or_else(|| {
            SessionError::BackendMissing(format!("no adapter for backend {}", handle.backend))
        })?;
        let projection = Projection::new(id, &handle.backend, adapter.projector());
        let version = projection.projector_version();
        let (projection, changed) = self
            .store
            .rebuild_projection(id.to_string(), projection)
            .await?;
        *lock(&handle.projection_version) = Some(version);
        {
            let mut hub = lock(&handle.hub);
            hub.last_seq = projection.last_seq();
            if changed {
                for subscriber in hub.subscribers.values() {
                    subscriber.overflowed.store(true, Ordering::SeqCst);
                    if !Arc::ptr_eq(&subscriber.kick, caller) {
                        subscriber.kick.send_replace(true);
                    }
                }
                hub.subscribers.clear();
            }
        }
        send_tile(&self.tiles, id, projection.tile());
        Ok(changed)
    }

    /// Subscribes a connection to a session's events from `from_seq` on
    /// (PROTO-CORE-API-R02). Returns the subscription, which stops when
    /// dropped, and the seq of the last event so far.
    ///
    /// Nothing is sent until [`Subscription::start`], so the caller can put
    /// its response on the connection first.
    pub fn subscribe(
        &self,
        id: &str,
        from_seq: u64,
        outbound: mpsc::Sender<String>,
        kick: Kick,
    ) -> Result<(Subscription, Option<u64>), SessionError> {
        let handle = self.handle(id)?;
        let Some(projection_version) = *lock(&handle.projection_version) else {
            return Err(SessionError::InvalidState(format!(
                "the events of session {id} cannot be read: Core has no adapter for backend {}, \
                 or could not repair its projection at startup",
                handle.backend
            )));
        };
        let (events_tx, events_rx) = mpsc::channel(SUBSCRIBER_QUEUE);
        let overflowed = Arc::new(AtomicBool::new(false));
        // Registered and `last_seq` read under one lock, the lock the
        // pipeline publishes under: every event up to `last_seq` is in the
        // cache, and every later one will arrive on the queue. That split is
        // what makes the stream gap-free and duplicate-free.
        let (subscriber, last_seq) = {
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
            (subscriber, hub.last_seq)
        };
        let (go_tx, go_rx) = oneshot::channel();
        let task = tokio::spawn(forward(Forward {
            store: self.store.clone(),
            session_id: id.to_string(),
            projection_version,
            from_seq,
            replay_to: last_seq,
            live: events_rx,
            outbound,
            overflowed,
            kick,
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

fn is_live(state: LifecycleState) -> bool {
    matches!(
        state,
        LifecycleState::Starting
            | LifecycleState::Running
            | LifecycleState::WaitingUser
            | LifecycleState::Idle
    )
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

/// Core's side of `AdapterSink` for one live session.
struct CoreSink {
    session_id: String,
    inner: Mutex<SinkInner>,
    /// Set by `session.stop`: the exit that follows is the user's doing.
    stop_requested: AtomicBool,
    /// Becomes true when the adapter reports the process exit, even one held
    /// back because the start had not finished.
    exit_seen: watch::Sender<bool>,
    /// Becomes true when the record that ends the session is queued.
    ended: watch::Sender<bool>,
}

struct SinkInner {
    next_index: u64,
    /// `None` once Core has closed the sink, which ends the pipeline.
    lines: Option<mpsc::UnboundedSender<RawLine>>,
    start_done: bool,
    /// An exit reported before the start finished, recorded when it does:
    /// the record that ends the session must come last.
    deferred_exit: Option<io::Result<ProcessExit>>,
    /// The ending record is in; nothing more from the adapter is recorded.
    ended: bool,
}

/// Why a start did not produce a running session.
struct StartFailure {
    message: String,
    stopped_by_user: bool,
}

impl CoreSink {
    fn new(session_id: &str, lines: mpsc::UnboundedSender<RawLine>) -> Self {
        CoreSink {
            session_id: session_id.to_string(),
            inner: Mutex::new(SinkInner {
                next_index: 0,
                lines: Some(lines),
                start_done: false,
                deferred_exit: None,
                ended: false,
            }),
            stop_requested: AtomicBool::new(false),
            exit_seen: watch::Sender::new(false),
            ended: watch::Sender::new(false),
        }
    }

    /// Stamps and queues one line. Called with the lock held, so indices
    /// follow the order of the calls.
    fn push(inner: &mut SinkInner, dir: Direction, line: String) {
        let raw = RawLine {
            index: inner.next_index,
            ts: now_rfc3339(),
            dir,
            line,
        };
        inner.next_index += 1;
        if let Some(lines) = &inner.lines {
            // Unbounded, so this never blocks the adapter's read loop. The
            // pipeline writes whole batches per transaction, which keeps up
            // with any backend; the queue only grows while the database
            // cannot be written at all.
            let _ = lines.send(raw);
        }
    }

    fn record_core(&self, record: &CoreRecord) {
        let mut inner = lock(&self.inner);
        Self::push(&mut inner, Direction::Core, record.to_line());
    }

    /// The index the next line will get.
    fn next_index(&self) -> u64 {
        lock(&self.inner).next_index
    }

    fn exit_record(&self, exit: io::Result<ProcessExit>) -> CoreRecord {
        let (code, error) = match exit {
            Ok(exit) => (exit.code, None),
            Err(e) => (None, Some(e.to_string())),
        };
        CoreRecord::ProcessExited {
            code,
            stopped_by_user: self.stop_requested.load(Ordering::SeqCst),
            error,
        }
    }

    /// The session task's report of how the start went. Records the line
    /// that ends the session when the start failed, or when the process
    /// already exited while it was starting.
    fn start_finished(&self, outcome: Result<(), StartFailure>) {
        let mut inner = lock(&self.inner);
        inner.start_done = true;
        let record = match outcome {
            Ok(()) => match inner.deferred_exit.take() {
                Some(exit) => self.exit_record(exit),
                None => return,
            },
            Err(failure) => CoreRecord::StartFailed {
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
        Self::push(&mut inner, Direction::Core, record.to_line());
        inner.ended = true;
        drop(inner);
        self.ended.send_replace(true);
    }

    /// Stops queueing lines, which ends the pipeline once it has stored the
    /// ones already queued.
    fn close(&self) {
        lock(&self.inner).lines = None;
    }
}

impl AdapterSink for CoreSink {
    fn record(&self, dir: Direction, line: String) {
        if dir == Direction::Core {
            tracing::warn!(session = %self.session_id, "an adapter recorded a core line; ignored");
            return;
        }
        let mut inner = lock(&self.inner);
        if inner.ended {
            // The adapter promised the exit comes after its last line.
            tracing::warn!(session = %self.session_id, "line recorded after the session ended; ignored");
            return;
        }
        Self::push(&mut inner, dir, line);
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
            let record = self.exit_record(exit);
            Self::push(&mut inner, Direction::Core, record.to_line());
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
    store: Store,
    tiles: Arc<TileHub>,
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
        store,
        tiles,
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
    sink.record_core(&CoreRecord::ProcessStarted {
        backend: handle.backend.clone(),
        backend_version,
    });

    let mut cancel = live.cancel.subscribe();
    let as_sink: Arc<dyn AdapterSink> = sink.clone();
    // The start runs in a task of its own, so an adapter that panics fails
    // the start instead of leaving the session live forever.
    let mut start = tokio::spawn(async move { adapter.start(transport, config, as_sink).await });
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
            // pipeline that gave up can no longer record anything, so the
            // backend is stopped rather than left running unrecorded.
            let mut ended = sink.ended.subscribe();
            let pipeline_result = tokio::select! {
                _ = ended.wait_for(|ended| *ended) => None,
                result = &mut pipeline => Some(result),
            };
            if let Some(result) = pipeline_result {
                pipeline_done = true;
                if let Err(e) = result {
                    tracing::error!(session = %handle.id, error = %e, "the session's pipeline failed");
                }
                tracing::error!(session = %handle.id, "the raw log cannot be written; stopping the backend");
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
    if pipeline_done {
        // Its ending could not be recorded, so its row still says it runs.
        // Say `error` instead, so the rail does not show a live session
        // nothing can stop; the next Core's repair re-derives the rest.
        if let Err(e) = store
            .set_session_state(handle.id.clone(), LifecycleState::Error)
            .await
        {
            tracing::error!(session = %handle.id, error = %e, "could not mark the session as failed");
        }
        // And the rail hears of it, as it hears of every state change.
        match store.session(handle.id.clone()).await {
            Ok(Some(row)) => send_tile(&tiles, &handle.id, &row.tile),
            Ok(None) => {}
            Err(e) => {
                tracing::error!(session = %handle.id, error = %e, "could not read the failed session's tile");
            }
        }
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
    projection: Projection,
    lines: mpsc::UnboundedReceiver<RawLine>,
    progress: watch::Sender<Progress>,
    tiles: Arc<TileHub>,
}

async fn run_pipeline(pipeline: Pipeline) {
    let Pipeline {
        store,
        handle,
        mut projection,
        mut lines,
        progress,
        tiles,
    } = pipeline;
    let projection_version = projection.projector_version();
    let mut last_tile = projection.tile().clone();
    // Ends once the sink is closed and every queued line has been taken.
    while let Some(first) = lines.recv().await {
        let mut bytes = first.line.len();
        let mut batch = vec![first];
        while batch.len() < BATCH && bytes < BATCH_BYTES {
            match lines.try_recv() {
                Ok(line) => {
                    bytes += line.line.len();
                    batch.push(line);
                }
                Err(_) => break,
            }
        }
        let next_line = batch.last().map_or(0, |line| line.index + 1);
        let events: Vec<Envelope> = batch.iter().flat_map(|l| projection.project(l)).collect();
        let write = Arc::new(LogBatch {
            session_id: handle.id.clone(),
            lines: batch,
            events,
            projection_version,
            progress: SessionProgress::of(&projection),
        });
        if !store_with_retry(&store, &write).await {
            // Nothing after this can be stored in order either. Ending here
            // tells the session task, which stops the backend.
            return;
        }

        publish(&handle, &write.events);
        if projection.tile() != &last_tile {
            last_tile = projection.tile().clone();
            send_tile(&tiles, &handle.id, &last_tile);
        }
        progress.send_replace(Progress {
            lines: next_line,
            turn: projection.turn(),
        });
    }
}

/// Writes `batch`, retrying a few times. `false` when it could not be
/// stored.
async fn store_with_retry(store: &Store, batch: &Arc<LogBatch>) -> bool {
    let mut delays = WRITE_RETRY.iter();
    loop {
        match store.append_session_log(Arc::clone(batch)).await {
            Ok(()) => return true,
            Err(e) => match delays.next() {
                Some(delay) => {
                    tracing::error!(session = %batch.session_id, error = %e, ?delay, "could not store raw log lines; retrying");
                    tokio::time::sleep(*delay).await;
                }
                None => {
                    tracing::error!(session = %batch.session_id, error = %e, "could not store raw log lines; giving up");
                    return false;
                }
            },
        }
    }
}

/// Offers committed events to every subscriber. A subscriber whose queue is
/// full is dropped, never waited for: one slow client must not stall the
/// session for everyone else.
fn publish(handle: &Handle, events: &[Envelope]) {
    let Some(last) = events.last() else {
        return;
    };
    // Serialized before taking the lock, so the lock is held only for the
    // hand-off.
    let published: Vec<Arc<Published>> = events
        .iter()
        .map(|event| {
            Arc::new(Published {
                seq: event.seq,
                line: event_line(&handle.id, event),
            })
        })
        .collect();
    let mut hub = lock(&handle.hub);
    hub.last_seq = Some(last.seq);
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

/// A `session.event` notification line.
fn event_line(session_id: &str, event: &Envelope) -> String {
    let params = SessionEventParams {
        session_id: session_id.to_string(),
        event: event.clone(),
    };
    notification_line("session.event", &params)
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
    store: Store,
    session_id: String,
    projection_version: u32,
    from_seq: u64,
    /// The last event already committed when the subscription began; those
    /// up to it are replayed from the cache, later ones arrive on `live`.
    replay_to: Option<u64>,
    live: mpsc::Receiver<Arc<Published>>,
    outbound: mpsc::Sender<String>,
    overflowed: Arc<AtomicBool>,
    kick: Kick,
    go: oneshot::Receiver<()>,
}

/// Replays, then forwards live events, to one connection. This task, not
/// the pipeline, waits for the client: `outbound.send` waits for room in
/// the connection's write queue.
async fn forward(task: Forward) {
    let Forward {
        store,
        session_id,
        projection_version,
        from_seq,
        replay_to,
        mut live,
        outbound,
        overflowed,
        kick,
        go,
    } = task;
    // Dropped unsent: the subscribe response never went out.
    if go.await.is_err() {
        return;
    }

    let mut next = from_seq;
    if let Some(last) = replay_to {
        while next <= last {
            let to = last.min(next.saturating_add(REPLAY_PAGE - 1));
            let page = match store
                .cached_events(
                    session_id.clone(),
                    projection_version,
                    next,
                    to,
                    REPLAY_BYTES,
                )
                .await
            {
                Ok(page) => page,
                Err(e) => {
                    // A gap would break the promise of the subscription;
                    // closing the connection tells the client to retry.
                    tracing::error!(session = %session_id, error = %e, "replaying events failed");
                    kick.send_replace(true);
                    return;
                }
            };
            if overflowed.load(Ordering::SeqCst) {
                // Dropped meanwhile (re-projected, or fell behind); the hub
                // closed the connection if it had to.
                return;
            }
            let mut progressed = false;
            for (seq, envelope) in &page {
                if *seq != next {
                    break;
                }
                let line = cached_event_line(&session_id, envelope);
                if outbound.send(line).await.is_err() {
                    return;
                }
                next += 1;
                progressed = true;
            }
            if !progressed {
                // The next event is not in the cache: the session was
                // removed or re-projected meanwhile. Skipping ahead would
                // leave a gap the client never hears about; closing the
                // connection tells it to resubscribe.
                tracing::warn!(session = %session_id, next, "the cache changed during a replay; disconnecting");
                kick.send_replace(true);
                return;
            }
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

/// A `session.event` line around an envelope stored as JSON, without
/// parsing it again.
fn cached_event_line(session_id: &str, envelope: &str) -> String {
    let session_id = serde_json::to_string(session_id).expect("strings serialize");
    format!(
        r#"{{"jsonrpc":"2.0","method":"session.event","params":{{"session_id":{session_id},"event":{envelope}}}}}"#
    )
}

/// Locks a std mutex, carrying on if a panicking thread poisoned it: every
/// value behind these locks stays consistent between single updates.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
