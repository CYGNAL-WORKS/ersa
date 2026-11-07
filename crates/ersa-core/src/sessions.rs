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

use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, BackendChoices, Direction, OpenElsewhere,
    PermissionDecision, ProcessExit, RawLine, RecordDeletion, RecordEvent, RecordSummary,
    SessionConfig, Transport,
};
use ersa_protocol::core_api::rpc::Notification;
use ersa_protocol::core_api::{
    Deleting, Section, Session, SessionEventParams, SessionTileParams, Tile,
};
use ersa_protocol::events::{
    Envelope, ErrorCode, ErrorEvent, Event, LifecycleState, NoticeCode, NoticeLevel, SessionNotice,
};
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::task::AbortHandle;

use crate::app_log::{AppLog, Facts, LifecycleFact};
use crate::persistence::{self, SessionRow, Store, now_rfc3339};
use crate::projection::View;
use crate::raw_log::RawLog;

mod send;

pub use send::Pacer;

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

/// How long asking an adapter whether another process has a session open
/// may take. The answer only informs the user, so a registry that hangs
/// makes it unknown rather than holding up what was asked for.
const OPEN_ELSEWHERE_CHECK: Duration = Duration::from_secs(5);

/// How many events a replay sends before it looks again for whether the
/// subscription is still wanted.
const REPLAY_PAGE: usize = 500;

/// The most bytes of event lines one open view holds. A long live session
/// with streaming replies emits a great many events; a client that asks for
/// ones older than these starts from the oldest held, as `first_seq` in the
/// subscribe answer says.
const MAX_VIEW_BYTES: usize = 64 * 1024 * 1024;

/// How long `session.send` waits for its message to be projected, to report
/// the turn it started.
const SEND_SETTLE: Duration = Duration::from_secs(10);

/// How long `session.remove` waits to remove the session's scratch folder.
const SCRATCH_REMOVAL: Duration = Duration::from_secs(30);

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
    /// The attachment at `index` of a `session.send` can't be sent.
    #[error("attachment {index}: {reason}")]
    AttachmentRefused { index: usize, reason: String },
    /// Core handed a message over but can't tell whether the backend took it.
    #[error("{0}")]
    DeliveryUnknown(String),
    /// A delete under way holds the session's workspace
    /// ([`SessionManager::claim_workspace`]).
    #[error("{0}")]
    Busy(String),
    /// A `session.arrange` named a section that does not exist.
    #[error("no section {0}")]
    NoSection(String),
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
    /// A pause after each chunk an attachment's copy writes, for tests.
    copy_pause: Duration,
    /// Set by [`SessionManager::stop_all`]; no session starts after it.
    /// Read and written under the `sessions` lock.
    closing: AtomicBool,
    /// The workspaces a `session.delete` under way has claimed, each with
    /// the session it deletes. In memory only, so a restart drops every
    /// claim (PROTO-CORE-API, `session.delete`).
    claims: Claims,
    /// Held while a session's arrangement is written to the store and to
    /// memory, so a `section.remove` and a `session.arrange` into that
    /// section land in the same order in both.
    arrangement: tokio::sync::Mutex<()>,
}

/// Claimed workspaces, by id, with the session each delete is for.
type Claims = Arc<Mutex<HashMap<String, String>>>;

/// A workspace claimed for one delete; dropping it releases the claim, so
/// every way out of a delete, a refusal or a failure included, releases it.
pub struct WorkspaceClaim {
    claims: Claims,
    workspace_id: String,
}

impl Drop for WorkspaceClaim {
    fn drop(&mut self) {
        lock(&self.claims).remove(&self.workspace_id);
    }
}

/// The refusal for a request a delete under way holds back.
fn busy(workspace_id: &str) -> SessionError {
    SessionError::Busy(format!(
        "a session in workspace {workspace_id} is being deleted; try again once it is done"
    ))
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
    /// Why the backend did not start, the last time Core started it: a
    /// message sent to the session is answered with it, so whoever sent the
    /// first message learns the backend's own reason (FLOW-NEW-SESSION-R08).
    start_failure: Mutex<Option<String>>,
    /// A read of the session's record still running after its caller gave
    /// up waiting; the next open waits on it rather than start another, so
    /// a file system that hangs gathers one stuck read per session, not one
    /// per attempt.
    record_read: ReadSlot<RecordReadResult>,
    /// The same for taking the record's version, to check whether a read an
    /// open joined still holds: every open that needs the check joins the
    /// one running, so on a disk that hangs for stats these checks hold one
    /// thread per session, as each slot here does.
    version_check: ReadSlot<Result<Option<String>, String>>,
    /// The same for reading the record's title and creation time.
    summary_read: ReadSlot<Result<Option<RecordSummary>, String>>,
    /// Set when the session is removed, for reads of its record that
    /// outlive it ([`read_unless_removed`]).
    removed: Arc<AtomicBool>,
    /// The send under way, if any: one at a time per session.
    sending: Mutex<Option<send::Sending>>,
    /// Held by whatever works in the session's scratch folder: a send's
    /// copying, and removing the folder with the session.
    scratch: Arc<tokio::sync::Mutex<()>>,
}

/// A read of a session's record: the record's version when the read began,
/// and its events.
type RecordReadResult = Result<(Option<String>, Arc<Vec<RecordEvent>>), String>;

/// Whether `read` still describes its record, whose version is `now`: the
/// version it was read at is the same. A read from before the record existed
/// (no version) does not hold once it does. A read that failed is reported
/// as it is.
fn still_holds(read: &RecordReadResult, now: Option<&str>) -> bool {
    match read {
        Ok((version, _)) => version.as_deref() == now,
        Err(_) => true,
    }
}

/// What `read`, a read of a session's record, returns; when the session was
/// removed while it ran, `forget` then lets go of what the adapter kept of
/// the record. A read that outlived its caller can end after `remove` had
/// the adapter forget the record, and would otherwise keep it again.
/// `remove` marks the session before it forgets, so either its forget comes
/// after anything this read kept, or this sees the mark.
async fn read_unless_removed<T>(
    read: impl Future<Output = T>,
    removed: &AtomicBool,
    forget: impl FnOnce(),
) -> T {
    let value = read.await;
    if removed.load(Ordering::SeqCst) {
        forget();
    }
    value
}

/// One read of a session's record that may still be running.
type ReadSlot<T> = tokio::sync::Mutex<Option<tokio::task::JoinHandle<T>>>;

/// The result of a read of the session's record, started with `start`
/// unless one is still running in `slot`, which is waited on instead: a
/// read is never started beside one that has not finished (a thread stuck on
/// a hung disk stays one thread). `None` when it takes longer than
/// `timeout`; it is kept for the next caller.
///
/// What a read returns is what the record held when that read began. A
/// caller that joined a read already running takes its result only when
/// `still_current` says the record has not changed since that read began,
/// and reads again otherwise. A read that finished after its caller gave up
/// is not handed on: nothing says when it was taken.
///
/// `still_current` looks at the record too, so it is bounded by `timeout` as
/// a read is, and runs without `slot` locked: a disk that stops answering
/// fails this open, and holds no other behind it.
async fn read_once<T: Send + 'static>(
    slot: &ReadSlot<T>,
    timeout: Duration,
    start: impl FnOnce() -> tokio::task::JoinHandle<T>,
    still_current: impl AsyncFnOnce(&T) -> bool,
) -> Option<T> {
    let mut start = Some(start);
    let first = {
        let mut slot = slot.lock().await;
        join_or_start(&mut slot, &mut start);
        wait(&mut slot, timeout).await?
    };
    // Started here: what it read is current.
    if start.is_none() {
        return Some(first);
    }
    // Joined a read already running: the record may have grown since that
    // read began.
    match tokio::time::timeout(timeout, still_current(&first)).await {
        Ok(true) => return Some(first),
        Ok(false) => {}
        Err(_) => return None,
    }
    let mut slot = slot.lock().await;
    // A read there now was started after this open began, so is current
    // for it.
    join_or_start(&mut slot, &mut start);
    wait(&mut slot, timeout).await
}

/// Leaves a read still running in `slot` to be joined, or puts one started
/// with `start` there. One that finished after its caller gave up is
/// dropped.
fn join_or_start<T>(
    slot: &mut Option<tokio::task::JoinHandle<T>>,
    start: &mut Option<impl FnOnce() -> tokio::task::JoinHandle<T>>,
) {
    if slot.as_ref().is_some_and(|task| task.is_finished()) {
        *slot = None;
    }
    if slot.is_none() {
        *slot = start.take().map(|start| start());
    }
}

/// Waits up to `timeout` for the read in `slot`, awaited where it lies, so a
/// caller that goes away (its connection closed) leaves it for the next one,
/// never a second read beside it. `None` (the read kept) when it is still
/// running, or (the read gone) when it failed.
async fn wait<T>(slot: &mut Option<tokio::task::JoinHandle<T>>, timeout: Duration) -> Option<T> {
    let task = slot.as_mut()?;
    match tokio::time::timeout(timeout, task).await {
        Ok(result) => {
            *slot = None;
            match result {
                Ok(value) => Some(value),
                Err(e) => {
                    tracing::error!(error = %e, "a session record read failed");
                    None
                }
            }
        }
        // Still running: kept for the next caller to wait on.
        Err(_) => None,
    }
}

/// The session's open view, and who receives its events.
#[derive(Default)]
struct Hub {
    /// `None` until the session is opened in this Core, and again once
    /// nobody reads a session Core is not running.
    view: Option<View>,
    /// The open view's identity, which `session.subscribe` answers with: a
    /// client that sees it change knows the events it holds belong to
    /// another view, and starts over.
    view_id: String,
    /// The view's events still held, as notification lines:
    /// `events[seq - first_seq]`. Only the latest [`MAX_VIEW_BYTES`] are held.
    events: VecDeque<Arc<Published>>,
    /// The `seq` of the first event held.
    first_seq: u64,
    /// Bytes of notification lines held.
    held_bytes: usize,
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
    /// The `seq` of the last event handed to subscribers; `None` before the
    /// first.
    fn last_seq(&self) -> Option<u64> {
        (self.first_seq + self.events.len() as u64).checked_sub(1)
    }

    /// Holds a published event, letting the oldest go past
    /// [`MAX_VIEW_BYTES`] (the latest is always held).
    fn hold(&mut self, event: Arc<Published>) {
        self.hold_within(event, MAX_VIEW_BYTES);
    }

    fn hold_within(&mut self, event: Arc<Published>, max_bytes: usize) {
        self.held_bytes += event.line.len();
        self.events.push_back(event);
        while self.held_bytes > max_bytes && self.events.len() > 1 {
            if let Some(old) = self.events.pop_front() {
                self.held_bytes -= old.line.len();
                self.first_seq += 1;
            }
        }
    }

    /// Makes `view`, whose events so far are `events`, the open view, under a
    /// new identity.
    fn install(&mut self, session_id: &str, view: View, events: &[Envelope]) {
        self.view = Some(view);
        self.view_id = persistence::new_id("v");
        self.events.clear();
        self.first_seq = 0;
        self.held_bytes = 0;
        for event in events {
            self.hold(Arc::new(published(session_id, event)));
        }
    }

    /// Lets the view and its events go.
    fn close(&mut self) {
        self.view = None;
        self.view_id.clear();
        self.events = VecDeque::new();
        self.first_seq = 0;
        self.held_bytes = 0;
    }
}

/// What `session.subscribe` answers with.
#[derive(Debug, Clone)]
pub struct Subscribed {
    /// The seq of the last event so far; `None` before the first.
    pub last_seq: Option<u64>,
    /// The identity of the view the subscription reads.
    pub view: String,
    /// The first event the view still holds, when it no longer holds them
    /// all from 0.
    pub first_seq: Option<u64>,
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
            start_failure: Mutex::new(None),
            record_read: tokio::sync::Mutex::new(None),
            version_check: tokio::sync::Mutex::new(None),
            summary_read: tokio::sync::Mutex::new(None),
            removed: Arc::default(),
            sending: Mutex::default(),
            scratch: Arc::default(),
        }
    }

    fn live(&self) -> Option<Arc<Live>> {
        lock(&self.live).clone()
    }

    /// The error for a request that needs the backend running when it is
    /// not: the reason it failed to start, when it did.
    fn not_running(&self) -> SessionError {
        match &*lock(&self.start_failure) {
            Some(message) => {
                SessionError::InvalidState(format!("the backend could not be started: {message}"))
            }
            None => not_running(),
        }
    }

    /// Lets the view and the raw log go when Core is not running the session
    /// and nobody reads it: a session opened again is projected afresh.
    fn close_if_idle(&self) {
        // Checked under the hub's lock, so a session that starts meanwhile is
        // seen as live rather than losing its view (live is only ever taken
        // after the hub, never before it).
        let mut hub = lock(&self.hub);
        if self.live().is_some() || !hub.subscribers.is_empty() || hub.view.is_none() {
            return;
        }
        hub.close();
        drop(hub);
        *lock(&self.raw) = RawLog::default();
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
                let (mut view, _) = View::open(&self.id, &self.backend, None, &[]);
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
            model: row.model,
            effort: row.effort,
            permission_mode: row.permission_mode.unwrap_or_default(),
            mode: row.mode,
            title: presented.title,
            state,
            created_at: presented.created_at,
            last_event_seq: hub.last_seq(),
            sending: lock(&self.sending).as_ref().map(send::Sending::phase),
            archived: row.archived,
            pinned: row.pinned,
            section_id: row.section_id,
            last_activity_at: row.last_activity_at,
            deleting: row.deleting,
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
            copy_pause: Duration::ZERO,
            closing: AtomicBool::new(false),
            claims: Arc::default(),
            arrangement: tokio::sync::Mutex::new(()),
        }
    }

    /// The same manager, pausing after each chunk an attachment's copy
    /// writes ([`crate::CoreConfig::copy_pause`]).
    pub fn with_copy_pause(mut self, copy_pause: Duration) -> Self {
        self.copy_pause = copy_pause;
        self
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
        let rows = self.store.sessions().await?;
        // Facts of sessions removed in an earlier run are not carried on.
        self.app_log
            .retain_only(rows.iter().map(|row| row.id.clone()).collect());
        for mut row in rows {
            // Sessions from before Core recorded the permission mode were
            // all started stating their backend's default.
            if row.permission_mode.is_none()
                && let Some(adapter) = self.adapters.get(&row.backend)
            {
                let default = BackendChoices::default_of(&adapter.choices().permission_modes);
                row.permission_mode = (!default.is_empty()).then_some(default);
            }
            let known = facts.get(&row.id);
            let started = known.map_or(UNKNOWN_TIME.to_string(), |f| f.first.ts.clone());
            let handle = Arc::new(Handle::new(row, started));
            match known.map(|f| &f.last) {
                Some(Some(last)) if !last.fact.ends() => {
                    let ts = now_rfc3339();
                    let fact = LifecycleFact::CoreRestarted;
                    self.app_log.write(&handle.id, &ts, &fact);
                    *lock(&handle.ending) = Some((fact, ts));
                }
                Some(Some(last)) => {
                    *lock(&handle.ending) = Some((last.fact.clone(), last.ts.clone()));
                }
                // Its last fact could not be read: how it ended is unknown,
                // and nothing is made up for it.
                Some(None) | None => {}
            }
            if let Some(adapter) = self.adapters.get(&handle.backend) {
                let row = lock(&handle.row).clone();
                if let (Some(backend_id), Some(transport)) =
                    (row.backend_session_id, transport_for(&row.host_id))
                {
                    let (handle, adapter) = (Arc::clone(&handle), Arc::clone(adapter));
                    summaries.spawn(async move {
                        apply_summary(&handle, adapter, transport, row.cwd, backend_id).await;
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

    /// Returns once every lifecycle fact written so far is on disk, for a
    /// Core about to exit.
    pub async fn flush_app_log(&self) {
        self.app_log.flushed().await;
    }

    /// Whether an adapter for `backend` is registered.
    pub fn has_backend(&self, backend: &str) -> bool {
        self.adapters.contains_key(backend)
    }

    /// The adapter registered for `backend`.
    pub fn adapter(&self, backend: &str) -> Option<Arc<dyn Adapter>> {
        self.adapters.get(backend).cloned()
    }

    /// Receives `session.tile` notifications from now on, the latest per
    /// session.
    pub fn tile_feed(&self) -> TileFeed {
        self.tiles.feed()
    }

    /// Every image type some backend on this host takes, for telling an
    /// image by its first bytes.
    pub fn image_types(&self) -> Vec<&'static str> {
        let mut types: Vec<&'static str> = self
            .adapters
            .values()
            .flat_map(|adapter| adapter.image_limits().media_types.iter().copied())
            .collect();
        types.sort_unstable();
        types.dedup();
        types
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
        // Only what was named is stored and reported (PROTO-CORE-API-R16).
        self.store.insert_session(row.clone()).await?;
        // Checked once the row exists, so a delete that claims the workspace
        // after this check counts the new session, and one that claimed it
        // before is seen here.
        if let Err(busy) = self.refuse_if_claimed(&row.workspace_id) {
            self.store.delete_session(row.id.clone()).await?;
            return Err(busy);
        }
        let handle = Arc::new(Handle::new(row, now_rfc3339()));
        lock(&handle.presented).title = title;
        // A new session has no record yet: the backend has not been told
        // anything, so its events are the live stream's from the start.
        let (view, events) =
            View::open(&handle.id, &handle.backend, Some(adapter.projector()), &[]);
        lock(&handle.hub).install(&handle.id, view, &events);
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
        self.refuse_if_deleting(&handle)?;
        let workspace_id = lock(&handle.row).workspace_id.clone();
        self.refuse_if_claimed(&workspace_id)?;
        // A session whose ending is already queued is only winding down: a
        // client may have seen `session.ended` before Core let it go, and
        // can remove it, so wait for it to finish. Waited for before taking
        // `op`, so nothing else on the session queues behind the wait.
        if let Some(live) = handle.live()
            && live.sink.has_ended()
            && wait_finished(&live, STOP_TIMEOUT).await.is_err()
        {
            return Err(SessionError::Internal(format!(
                "session {id} has ended, but Core has not finished with it within {STOP_TIMEOUT:?}"
            )));
        }
        let _op = handle.op.lock().await;
        if handle.live().is_some() {
            return Err(SessionError::InvalidState(format!(
                "session {id} is running; stop it first"
            )));
        }
        let session = handle.describe();
        self.store.delete_session(id.to_string()).await?;
        self.let_go(&handle).await;
        Ok(session)
    }

    /// Lets go of a session whose row is gone: its scratch folder, what the
    /// app log, the adapter and the tiles keep of it, and its subscribers.
    async fn let_go(&self, handle: &Arc<Handle>) {
        let id = handle.id.as_str();
        // Its scratch folder goes with it (PROTO-CORE-API-R28), once no
        // copying holds it. Never past a bound: a file system that hangs
        // leaves the folder behind, logged, rather than hold up the
        // removal.
        let cwd = lock(&handle.row).cwd.clone();
        let scratch = Arc::clone(&handle.scratch);
        let session_id = id.to_string();
        // The scratch folder is waited for here, never on a thread.
        let removal = tokio::time::timeout(SCRATCH_REMOVAL, async move {
            let held = scratch.lock_owned().await;
            tokio::task::spawn_blocking(move || {
                let _held = held;
                crate::attachments::remove_scratch(Path::new(&cwd), &session_id)
            })
            .await
        });
        match removal.await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(e))) => {
                tracing::warn!(session = %id, error = %e, "removing the scratch folder failed");
            }
            Ok(Err(e)) => {
                tracing::warn!(session = %id, error = %e, "removing the scratch folder failed")
            }
            Err(_) => tracing::warn!(session = %id, "removing the scratch folder took too long"),
        }
        lock(&self.sessions).remove(id);
        self.app_log.forget(id);
        // What the adapter kept of its record goes too, and what a read
        // still running keeps once it ends.
        handle.removed.store(true, Ordering::SeqCst);
        let row = lock(&handle.row).clone();
        if let (Some(backend_id), Some(adapter)) =
            (&row.backend_session_id, self.adapters.get(&row.backend))
        {
            adapter.forget_record(&row.cwd, backend_id);
        }
        // A tile still waiting to go out would show the session again.
        self.tiles.forget(id);
        // Dropping the senders ends every subscription's forwarding task.
        // The connections stay open: the client that removed the session
        // may be one of them, and the session is gone from `session.list`.
        lock(&handle.hub).subscribers.clear();
    }

    /// Renames the session in its backend (PROTO-CORE-API-R12): through the
    /// running backend when Core has it live, else in the backend's session
    /// record on the session's host, reached by `transport_for`. The title
    /// is then the backend's, so Core shows it without reading the record
    /// again and stores none of its own.
    pub async fn set_title(
        &self,
        id: &str,
        title: &str,
        transport_for: impl FnOnce(&str) -> Option<Arc<dyn Transport>>,
    ) -> Result<Session, SessionError> {
        let handle = self.handle(id)?;
        let unnamed = || {
            SessionError::InvalidState(
                "the session's backend has not named it yet, so there is nothing to rename".into(),
            )
        };
        if let Some(live) = handle.live() {
            let session = live.wait_ready().await?;
            // Read once ready: the backend names the session as it starts.
            let backend_id = lock(&handle.row)
                .backend_session_id
                .clone()
                .ok_or_else(unnamed)?;
            let title = title.to_string();
            // A backend that is stopping refuses (not running) while it may
            // still be writing its record, so a refusal never falls back to
            // writing the record beside it: the user renames again once the
            // session has ended.
            bounded(async move { session.rename(&backend_id, &title).await }).await?;
        } else {
            let row = lock(&handle.row).clone();
            let backend_id = row.backend_session_id.ok_or_else(unnamed)?;
            let adapter = self.adapters.get(&row.backend).cloned().ok_or_else(|| {
                SessionError::BackendMissing(format!("no adapter for backend {}", row.backend))
            })?;
            let transport = transport_for(&row.host_id).ok_or_else(|| {
                SessionError::Adapter(AdapterError::Io(io::Error::new(
                    io::ErrorKind::NotConnected,
                    format!("host {} cannot be reached", row.host_id),
                )))
            })?;
            // Appended whatever the answer: the other process shows the new
            // title once it reads its record again (PROTO-CLAUDE-CODE-STREAM,
            // Renaming), so the user is told rather than refused.
            notice_if_open_elsewhere(
                &handle,
                Arc::clone(&adapter),
                Arc::clone(&transport),
                &backend_id,
                "may show the old title, and write it again, until it next reads the session",
            )
            .await;
            let (cwd, title) = (row.cwd, title.to_string());
            let renamed = bounded(async move {
                adapter
                    .rename_record(transport, &cwd, &backend_id, &title)
                    .await
            })
            .await;
            match renamed {
                Err(SessionError::Adapter(AdapterError::Io(e)))
                    if e.kind() == io::ErrorKind::NotFound =>
                {
                    // Never written, or deleted by the backend's retention.
                    return Err(SessionError::InvalidState(
                        "the backend has no record of this session to rename".into(),
                    ));
                }
                Err(SessionError::Adapter(AdapterError::Io(e)))
                    if e.kind() == io::ErrorKind::ResourceBusy =>
                {
                    // Something is writing the record right now.
                    return Err(SessionError::InvalidState(e.to_string()));
                }
                renamed => renamed?,
            }
        }
        lock(&handle.presented).title = Some(title.to_string());
        Ok(handle.describe())
    }

    /// Archives the session, or unarchives it (ADR 0026,
    /// FLOW-ARCHIVE-DELETE-SESSION). Archiving stops a live session first,
    /// as `session.stop` does, and marks it only once it has stopped: a stop
    /// that fails leaves the session as it was. Unarchiving leaves the
    /// session stopped; nothing restarts it.
    pub async fn set_archived(&self, id: &str, archived: bool) -> Result<Session, SessionError> {
        let handle = self.handle(id)?;
        self.refuse_if_deleting(&handle)?;
        if archived {
            self.stop(id).await?;
        }
        // Taken after the stop, which takes it itself. Held while marking,
        // so two marks of one session land in the store and in memory in
        // the same order, and a `session.remove` (which also holds it) is
        // either finished, and seen here, or waits for the mark.
        let _op = handle.op.lock().await;
        if handle.removed.load(Ordering::SeqCst) {
            return Err(SessionError::NotFound(id.to_string()));
        }
        // Stored first, then kept in memory: a store that fails leaves both
        // as they were, so `session.list` never reports what a restart would
        // lose.
        self.store
            .set_session_archived(id.to_string(), archived)
            .await?;
        lock(&handle.row).archived = archived;
        Ok(handle.describe())
    }

    /// Pins or unpins the session and moves it between sections, changing
    /// only what is given (ADR 0026, FLOW-ORGANIZE-SESSIONS). A section that
    /// does not exist is `not_found`, and changes nothing.
    pub async fn arrange(
        &self,
        id: &str,
        pinned: Option<bool>,
        section_id: Option<Option<String>>,
    ) -> Result<Session, SessionError> {
        let handle = self.handle(id)?;
        // As `set_archived`: a `session.remove` is either finished, and seen
        // here, or waits for the change. A delete marks the session under
        // the same lock, so the refusal below sees a mark made meanwhile.
        let _op = handle.op.lock().await;
        if handle.removed.load(Ordering::SeqCst) {
            return Err(SessionError::NotFound(id.to_string()));
        }
        self.refuse_if_deleting(&handle)?;
        let _arrangement = self.arrangement.lock().await;
        // Stored first, then kept in memory, so `session.list` never reports
        // what a restart would lose.
        let stored = self
            .store
            .arrange_session(id.to_string(), pinned, section_id.clone())
            .await?;
        if !stored {
            let missing = section_id.flatten().unwrap_or_default();
            return Err(SessionError::NoSection(missing));
        }
        {
            let mut row = lock(&handle.row);
            if let Some(pinned) = pinned {
                row.pinned = pinned;
            }
            if let Some(section_id) = section_id {
                row.section_id = section_id;
            }
        }
        Ok(handle.describe())
    }

    /// Removes section `id`, putting its sessions in no section
    /// (PROTO-CORE-API-R39). Returns the section as it was; `None` when
    /// there is no such section.
    pub async fn remove_section(&self, id: &str) -> Result<Option<Section>, SessionError> {
        let _arrangement = self.arrangement.lock().await;
        let removed = self.store.delete_section(id.to_string()).await?;
        if removed.is_some() {
            let handles: Vec<Arc<Handle>> = lock(&self.sessions).values().cloned().collect();
            for handle in handles {
                let mut row = lock(&handle.row);
                if row.section_id.as_deref() == Some(id) {
                    row.section_id = None;
                }
            }
        }
        Ok(removed)
    }

    // -- Deleting (PROTO-CORE-API, `session.delete`) --------------------------

    /// The session's row as last written.
    pub fn row(&self, id: &str) -> Result<SessionRow, SessionError> {
        Ok(lock(&self.handle(id)?.row).clone())
    }

    /// Refuses a delete while the session is starting, a turn is running
    /// in it (one waiting on the user included), or a message is being
    /// sent to it (FLOW-ARCHIVE-DELETE-SESSION-R05). `session.delete` asks
    /// once it holds the claim, so a send cannot start in between.
    pub fn check_deletable(&self, id: &str) -> Result<(), SessionError> {
        let handle = self.handle(id)?;
        let session = handle.describe();
        let working = matches!(
            session.state,
            LifecycleState::Starting | LifecycleState::Running | LifecycleState::WaitingUser
        );
        // A pending permission or question is a turn waiting on the user,
        // whatever state the backend last reported.
        let waiting = !session.tile.pending.is_empty();
        if (handle.live().is_some() && (working || waiting)) || session.sending.is_some() {
            return Err(SessionError::InvalidState(format!(
                "session {id} is starting or working; stop it before deleting it"
            )));
        }
        Ok(())
    }

    /// Claims workspace `workspace_id` for deleting session `session_id`,
    /// in memory only. Refused while another delete holds it.
    pub fn claim_workspace(
        &self,
        workspace_id: &str,
        session_id: &str,
    ) -> Result<WorkspaceClaim, SessionError> {
        let mut claims = lock(&self.claims);
        if claims.contains_key(workspace_id) {
            return Err(busy(workspace_id));
        }
        claims.insert(workspace_id.to_string(), session_id.to_string());
        Ok(WorkspaceClaim {
            claims: Arc::clone(&self.claims),
            workspace_id: workspace_id.to_string(),
        })
    }

    /// Refuses a request that would change workspace `workspace_id` while a
    /// delete holds it.
    fn refuse_if_claimed(&self, workspace_id: &str) -> Result<(), SessionError> {
        if lock(&self.claims).contains_key(workspace_id) {
            return Err(busy(workspace_id));
        }
        Ok(())
    }

    /// Refuses a request about session `handle` while a delete is under
    /// way for it, or it is marked deleting.
    fn refuse_if_deleting(&self, handle: &Handle) -> Result<(), SessionError> {
        let (workspace_id, deleting) = {
            let row = lock(&handle.row);
            (row.workspace_id.clone(), row.deleting)
        };
        if lock(&self.claims).get(&workspace_id) == Some(&handle.id) {
            return Err(busy(&workspace_id));
        }
        if deleting.is_some() {
            return Err(SessionError::InvalidState(format!(
                "session {} is being deleted; delete it again to finish",
                handle.id
            )));
        }
        Ok(())
    }

    /// Whether a process other than Core's own has the session open, as
    /// its backend's registry says, on the host `transport` reaches. Asked
    /// once Core's own process for it has stopped, so any process the
    /// registry names is another one. A session its backend never named has
    /// no record anyone could have open.
    pub async fn open_elsewhere(
        &self,
        id: &str,
        transport: Arc<dyn Transport>,
    ) -> Result<OpenElsewhere, SessionError> {
        let row = self.row(id)?;
        let Some(backend_id) = row.backend_session_id else {
            return Ok(OpenElsewhere::No);
        };
        let adapter = self.adapter(&row.backend).ok_or_else(|| {
            SessionError::BackendMissing(format!("no adapter for backend {}", row.backend))
        })?;
        Ok(ask_open_elsewhere(adapter, transport, &backend_id).await)
    }

    /// Marks the session deleting, `running`, and records whether its
    /// delete takes the workspace's files: `files`, unless an earlier try
    /// recorded that they stay. Returns what is recorded. Stored first, then
    /// kept in memory, as an archive mark is.
    pub async fn mark_deleting(&self, id: &str, files: bool) -> Result<bool, SessionError> {
        let handle = self.handle(id)?;
        let _op = handle.op.lock().await;
        if handle.removed.load(Ordering::SeqCst) {
            return Err(SessionError::NotFound(id.to_string()));
        }
        let recorded = self
            .store
            .mark_session_deleting(id.to_string(), files)
            .await?;
        lock(&handle.row).deleting = Some(Deleting::Running);
        Ok(recorded)
    }

    /// Marks the session's delete failed, for the user to try again.
    pub async fn mark_delete_failed(&self, id: &str) -> Result<(), SessionError> {
        let handle = self.handle(id)?;
        self.store.set_session_delete_failed(id.to_string()).await?;
        lock(&handle.row).deleting = Some(Deleting::Failed);
        Ok(())
    }

    /// Deletes the session's record from its backend, on the host
    /// `transport` reaches (PROTO-CORE-API-R36). A session its backend never
    /// named has no record to delete.
    pub async fn delete_record(
        &self,
        id: &str,
        transport: Arc<dyn Transport>,
    ) -> Result<RecordDeletion, SessionError> {
        let row = self.row(id)?;
        let Some(backend_id) = row.backend_session_id else {
            return Ok(RecordDeletion::Deleted);
        };
        let adapter = self.adapter(&row.backend).ok_or_else(|| {
            SessionError::BackendMissing(format!("no adapter for backend {}", row.backend))
        })?;
        let cwd = row.cwd;
        let call =
            tokio::spawn(async move { adapter.delete_record(transport, &cwd, &backend_id).await });
        match tokio::time::timeout(RECORD_READ, call).await {
            Ok(Ok(deleted)) => deleted.map_err(SessionError::Adapter),
            Ok(Err(e)) => Err(SessionError::Internal(format!("the adapter failed: {e}"))),
            Err(_) => Err(SessionError::Internal(format!(
                "deleting the session's record took longer than {RECORD_READ:?}"
            ))),
        }
    }

    /// Removes a deleted session, and with it its workspace's row when
    /// `workspace_id` names one, in one transaction; then lets go of it as
    /// `session.remove` does.
    pub async fn finish_delete(
        &self,
        id: &str,
        workspace_id: Option<String>,
    ) -> Result<(), SessionError> {
        let handle = self.handle(id)?;
        let _op = handle.op.lock().await;
        self.store
            .finish_session_delete(id.to_string(), workspace_id)
            .await?;
        self.let_go(&handle).await;
        Ok(())
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

    /// Interrupts the running turn, or cancels a message not yet handed to
    /// the backend instead (PROTO-CORE-API-R25).
    pub async fn interrupt(&self, id: &str) -> Result<(), SessionError> {
        if send::cancel(&*self.handle(id)?) {
            return Ok(());
        }
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
    /// (PROTO-CORE-API-R14), opening the session's view first when it needs
    /// one (see the module's notes). `transport_for` reaches a host, to read
    /// the session record there. Returns the subscription, which stops when
    /// dropped, and what the subscribe answer says.
    ///
    /// A request from before the first event the view holds starts there
    /// instead, and the answer says so (`first_seq`).
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
    ) -> Result<(Subscription, Subscribed), SessionError> {
        let handle = self.handle(id)?;
        // A session Core is not running gets a fresh view when it has none,
        // or when a client starts from the beginning with nobody else
        // reading (so turns taken elsewhere show). A live view is never
        // replaced.
        let wants_view = |hub: &Hub, live: bool| {
            !live && (hub.view.is_none() || (from_seq == 0 && hub.subscribers.is_empty()))
        };
        let (events_tx, events_rx) = mpsc::channel(SUBSCRIBER_QUEUE);
        let overflowed = Arc::new(AtomicBool::new(false));
        let subscriber = Subscriber {
            events: events_tx,
            overflowed: Arc::clone(&overflowed),
            kick: Arc::clone(&kick),
        };
        let mut opened: Option<(View, Vec<Envelope>)> = None;
        let mut installed = false;
        let mut subscriber = Some(subscriber);
        let (subscriber, subscribed, backlog) = loop {
            {
                let mut hub = lock(&handle.hub);
                let live = handle.live().is_some();
                // A view just opened goes in only if it is still wanted: a
                // client that subscribed to the open one meanwhile keeps it.
                if let Some((view, events)) = opened.take()
                    && wants_view(&hub, live)
                {
                    hub.install(&handle.id, view, &events);
                    installed = true;
                }
                // Registered and the backlog taken under one lock, the lock
                // the pipeline publishes under: every event up to `last_seq`
                // is in the backlog, and every later one will arrive on the
                // queue. That split is what makes the stream gap-free and
                // duplicate-free.
                if hub.view.is_some() && (installed || !wants_view(&hub, live)) {
                    let subscriber = subscriber.take().expect("registered once");
                    break register(&mut hub, subscriber, from_seq);
                }
            }
            // Opened outside the hub's lock, one opening at a time.
            let _op = handle.op.lock().await;
            if handle.live().is_some() || !wants_view(&lock(&handle.hub), false) {
                continue;
            }
            opened = Some(self.open_view(&handle, &transport_for).await?);
        };
        if installed {
            let session = handle.describe();
            send_tile(
                &self.tiles,
                &handle.id,
                &session.tile,
                session.last_activity_at,
            );
        }
        let first_seq = subscribed.first_seq.unwrap_or(0).max(from_seq);
        let (go_tx, go_rx) = oneshot::channel();
        let task = tokio::spawn(forward(Forward {
            from_seq: first_seq,
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
        Ok((subscription, subscribed))
    }

    /// A fresh view of a session this Core is not running: the events of its
    /// record, then how Core last saw its backend end, if it did.
    async fn open_view(
        &self,
        handle: &Arc<Handle>,
        transport_for: &impl Fn(&str) -> Option<Arc<dyn Transport>>,
    ) -> Result<(View, Vec<Envelope>), SessionError> {
        let row = lock(&handle.row).clone();
        let Some(adapter) = self.adapters.get(&row.backend).cloned() else {
            return Err(SessionError::InvalidState(format!(
                "the events of session {} cannot be read: Core has no adapter for backend {}",
                row.id, row.backend
            )));
        };
        let record = match &row.backend_session_id {
            // The backend never announced the session, so it has no record.
            None => Arc::default(),
            Some(backend_id) => {
                let read = match transport_for(&row.host_id) {
                    Some(transport) => {
                        let (reader, cwd, id) =
                            (Arc::clone(&adapter), row.cwd.clone(), backend_id.clone());
                        let at = Arc::clone(&transport);
                        let removed = Arc::clone(&handle.removed);
                        // The record's version is taken before it is read, so
                        // events read are never newer than the version they
                        // are kept under.
                        let start = move || {
                            tokio::spawn(async move {
                                let read = async {
                                    let version = reader
                                        .record_version(Arc::clone(&at), &cwd, &id)
                                        .await
                                        .ok()
                                        .flatten();
                                    reader
                                        .read_record(at, &cwd, &id)
                                        .await
                                        .map(|events| (version, events))
                                        .map_err(|e| e.to_string())
                                };
                                read_unless_removed(read, &removed, || {
                                    reader.forget_record(&cwd, &id)
                                })
                                .await
                            })
                        };
                        // A read this open joined still holds if the record's
                        // version is the one it was read at. One that failed
                        // is reported as it is, without looking again; a
                        // version that cannot be taken now is a change.
                        let still_current = async |read: &RecordReadResult| {
                            if read.is_err() {
                                return true;
                            }
                            let (adapter, cwd, id) =
                                (Arc::clone(&adapter), row.cwd.clone(), backend_id.clone());
                            let check = move || {
                                tokio::spawn(async move {
                                    adapter
                                        .record_version(transport, &cwd, &id)
                                        .await
                                        .map_err(|e| e.to_string())
                                })
                            };
                            // A check that began before this open may predate a
                            // change, so a joiner takes the next one (`false`).
                            // The open's own timeout around this check bounds it
                            // and leaves the check running for the next open; one
                            // here too could run out first and read again, to
                            // stall on the same disk.
                            let unbounded = Duration::MAX;
                            match read_once(&handle.version_check, unbounded, check, async |_| {
                                false
                            })
                            .await
                            {
                                Some(Ok(now)) => still_holds(read, now.as_deref()),
                                Some(Err(_)) | None => false,
                            }
                        };
                        read_once(&handle.record_read, RECORD_READ, start, still_current)
                            .await
                            .map(|read| read.map(|(_, events)| events))
                            .unwrap_or_else(|| {
                                Err(format!(
                                    "it is still being read after {RECORD_READ:?}, or the read failed"
                                ))
                            })
                    }
                    None => Err(format!("host {} cannot be reached", row.host_id)),
                };
                // A record that cannot be read is something the user should
                // see, in the session, rather than a refused subscription.
                read.unwrap_or_else(|e| {
                    tracing::warn!(session = %row.id, error = %e, "could not read the session record");
                    Arc::new(vec![RecordEvent {
                        message_id: None,
                        ts: Some(now_rfc3339()),
                        backend_version: None,
                        event: Event::Error(ErrorEvent {
                            code: ErrorCode::Transport,
                            message: format!("the session's history could not be read: {e}"),
                            recoverable: true,
                            action: None,
                        }),
                    }])
                })
            }
        };
        // Turns taken elsewhere may have changed its title too.
        if let (Some(backend_id), Some(transport)) =
            (row.backend_session_id.clone(), transport_for(&row.host_id))
        {
            apply_summary(handle, adapter, transport, row.cwd.clone(), backend_id).await;
        }
        let (mut view, mut events) = View::open(&row.id, &row.backend, None, &record);
        if let Some((fact, ts)) = lock(&handle.ending).clone() {
            events.extend(view.lifecycle(&fact, &ts));
        }
        Ok((view, events))
    }
}

/// Registers `subscriber` on the hub's open view and takes its backlog from
/// `from_seq` (or the first event held, if later).
fn register(
    hub: &mut Hub,
    subscriber: Subscriber,
    from_seq: u64,
) -> (u64, Subscribed, Vec<Arc<Published>>) {
    let id = hub.next_subscriber;
    hub.next_subscriber += 1;
    hub.subscribers.insert(id, subscriber);
    let skip = usize::try_from(from_seq.saturating_sub(hub.first_seq)).unwrap_or(usize::MAX);
    let backlog = hub.events.iter().skip(skip).cloned().collect();
    let subscribed = Subscribed {
        last_seq: hub.last_seq(),
        view: hub.view_id.clone(),
        first_seq: (hub.first_seq > 0).then_some(hub.first_seq),
    };
    (id, subscribed, backlog)
}

/// Reads the session's title and creation time from its record, keeping
/// what was known when the record has none or cannot be read. One read at
/// a time per session ([`read_once`]).
async fn apply_summary(
    handle: &Handle,
    adapter: Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
    cwd: String,
    backend_id: String,
) {
    let removed = Arc::clone(&handle.removed);
    let start = move || {
        tokio::spawn(async move {
            let read = adapter.summarize_record(transport, &cwd, &backend_id);
            read_unless_removed(read, &removed, || adapter.forget_record(&cwd, &backend_id))
                .await
                .map_err(|e| e.to_string())
        })
    };
    // A summary is small and the adapter keeps them by the record's state,
    // so a joiner simply reads again.
    match read_once(&handle.summary_read, SUMMARY_READ, start, async |_| false).await {
        Some(Ok(Some(summary))) => {
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
        Some(Ok(None)) => {}
        Some(Err(e)) => {
            tracing::warn!(session = %handle.id, error = %e, "could not read the session record's title");
        }
        None => {
            tracing::warn!(session = %handle.id, "reading the session record's title took too long");
        }
    }
}

/// Whether `text` has the shape of an RFC 3339 UTC time:
/// `YYYY-MM-DDTHH:MM:SS`, an optional fraction, then `Z`. That is the form
/// Core issues, so session times sort as text the way they sort as times.
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
        && at(10, b'T')
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
    rest == "Z"
}

/// Asks the adapter whether a process other than this Core's has the
/// session open, before Core resumes it or writes to its record, and when
/// one has, or the adapter cannot tell, adds a `session.notice` saying so to
/// the session's open view (PROTO-AGENT-EVENTS-R14). It never stops what
/// follows. `consequence` says what the other process does meanwhile.
///
/// Notices are not part of the record, so one reaches the clients reading
/// the session now; with none reading, there is nobody to tell.
/// Asks `adapter` whether another process has session `backend_id` open,
/// for at most [`OPEN_ELSEWHERE_CHECK`]: a check that panics or hangs could
/// not tell, so its answer is unknown.
async fn ask_open_elsewhere(
    adapter: Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
    backend_id: &str,
) -> OpenElsewhere {
    let id = backend_id.to_string();
    let check = tokio::spawn(async move { adapter.open_elsewhere(transport, &id).await });
    match tokio::time::timeout(OPEN_ELSEWHERE_CHECK, check).await {
        Ok(Ok(answer)) => answer,
        Ok(Err(_)) | Err(_) => OpenElsewhere::Unknown,
    }
}

async fn notice_if_open_elsewhere(
    handle: &Handle,
    adapter: Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
    backend_id: &str,
    consequence: &str,
) {
    let notice = match ask_open_elsewhere(adapter, transport, backend_id).await {
        OpenElsewhere::No => return,
        OpenElsewhere::Yes { pid } => SessionNotice {
            level: NoticeLevel::Warning,
            code: NoticeCode::SessionOpenElsewhere,
            message: format!(
                "Another process (pid {pid}) has this session open, and {consequence}."
            ),
            pid: Some(pid),
        },
        OpenElsewhere::Unknown => SessionNotice {
            level: NoticeLevel::Info,
            code: NoticeCode::SessionOpenCheckFailed,
            message: "Ersa could not tell whether another process has this session open.".into(),
            pid: None,
        },
    };
    let mut hub = lock(&handle.hub);
    let Some(view) = hub.view.as_mut() else {
        tracing::info!(session = %handle.id, code = ?notice.code, "no one is reading the session to tell");
        return;
    };
    let event = view.notice(notice, &now_rfc3339());
    publish(&mut hub, &handle.id, &[event]);
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
    /// How many lines Core has written to the backend, as the adapter
    /// recorded them before writing.
    host_lines: u64,
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
                host_lines: 0,
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
        if dir == Direction::Host {
            inner.host_lines += 1;
        }
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

    /// Whether the fact that ends the session is queued. Read under the lock
    /// it is queued under, so it holds for anyone who has seen that fact.
    fn has_ended(&self) -> bool {
        lock(&self.inner).ended
    }

    /// The index the next line will get.
    fn next_index(&self) -> u64 {
        lock(&self.inner).next_index
    }

    /// How many lines have been written to the backend.
    fn host_lines(&self) -> u64 {
        lock(&self.inner).host_lines
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
    // On disk before the process exists, so a crash just after starting it
    // still leaves the fact startup repairs from.
    sink.app_log.flushed().await;

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
            *lock(&handle.start_failure) = None;
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
            *lock(&handle.start_failure) = Some(e.to_string());
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
    *lock(&handle.live) = None;
    // Nobody reading it: nothing more will be added, so it goes.
    handle.close_if_idle();
    finished.send_replace(true);
    // The record now holds the session's turns, and with them its title.
    // Read after the stop has its answer, never on its way.
    let row = lock(&handle.row).clone();
    if let Some(backend_id) = row.backend_session_id {
        tokio::spawn(async move {
            apply_summary(&handle, adapter, transport, row.cwd, backend_id).await;
        });
    }
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
        let (events, tile, turn, announced) = {
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
            (events, tile, turn, announced)
        };

        // The backend's own id finds the session's record later; it is the
        // one thing about the session's history Ersa keeps. Stored before
        // anyone hears of the session's events, so a crash after a client
        // saw them cannot leave a session whose record is lost.
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
        // When a turn ends, its time is the session's last activity, and
        // only then (PROTO-CORE-API-R37). Stored before the turn's end is
        // published, as the backend's id is, so a client that saw it end
        // finds the time in `session.list`.
        let ended = events
            .iter()
            .rev()
            .find_map(|envelope| match envelope.event {
                Event::TurnCompleted(_) => Some(envelope.ts.clone()),
                _ => None,
            });
        let mut activity_changed = false;
        if let Some(at) = ended {
            match store
                .set_session_last_activity(handle.id.clone(), at.clone())
                .await
            {
                Ok(()) => {
                    lock(&handle.row).last_activity_at = Some(at);
                    activity_changed = true;
                }
                Err(e) => {
                    tracing::error!(session = %handle.id, error = %e, "could not record when a turn ended");
                }
            }
        }
        publish(&mut lock(&handle.hub), &handle.id, &events);
        // A tile carries the last activity, so a turn's end sends one even
        // when the tile itself ends as it began, as a turn that starts and
        // ends in one batch does.
        if tile != last_tile || activity_changed {
            let last_activity_at = lock(&handle.row).last_activity_at.clone();
            send_tile(&tiles, &handle.id, &tile, last_activity_at);
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
    for event in &published {
        hub.hold(Arc::clone(event));
    }
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

/// Sends a session's tile with its last activity, which every tile carries
/// as `session.list` has it.
fn send_tile(tiles: &TileHub, session_id: &str, tile: &Tile, last_activity_at: Option<String>) {
    let params = SessionTileParams {
        session_id: session_id.to_string(),
        tile: tile.clone(),
        last_activity_at,
    };
    tiles.publish(
        session_id,
        Latest::Tile,
        notification_line("session.tile", &params),
    );
}

/// Hands `session.tile` and `session.send_progress` notifications to every
/// attached connection, keeping only the latest per session (and kind) for
/// each: a tile is the whole state,
/// so a connection busy with a long replay gets the newest one when it is
/// ready, never a backlog, and is never closed for it.
#[derive(Default)]
struct TileHub {
    inboxes: Mutex<Vec<Weak<TileInbox>>>,
}

#[derive(Default)]
struct TileInbox {
    /// The newest line of each kind per session not yet taken.
    latest: Mutex<HashMap<(String, Latest), Arc<String>>>,
    ready: Notify,
}

impl TileHub {
    fn feed(&self) -> TileFeed {
        let inbox = Arc::new(TileInbox::default());
        lock(&self.inboxes).push(Arc::downgrade(&inbox));
        TileFeed { inbox }
    }

    /// Drops any tile or send progress for `session_id` not yet taken, for a
    /// removed session.
    fn forget(&self, session_id: &str) {
        self.forget_one(session_id, Latest::Tile);
        self.forget_one(session_id, Latest::SendProgress);
    }

    /// Drops the session's line of one kind not yet taken.
    fn forget_one(&self, session_id: &str, kind: Latest) {
        let key = (session_id.to_string(), kind);
        for inbox in lock(&self.inboxes).iter().filter_map(Weak::upgrade) {
            lock(&inbox.latest).remove(&key);
        }
    }

    fn publish(&self, session_id: &str, kind: Latest, line: String) {
        let line = Arc::new(line);
        let mut inboxes = lock(&self.inboxes);
        // A connection that has gone dropped its feed; forget its inbox.
        inboxes.retain(|inbox| {
            let Some(inbox) = inbox.upgrade() else {
                return false;
            };
            lock(&inbox.latest).insert((session_id.to_string(), kind), Arc::clone(&line));
            // Stores a permit when nobody waits, so a wake is never lost.
            inbox.ready.notify_one();
            true
        });
    }
}

/// The kinds of notification the hub keeps only the newest of, per session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Latest {
    Tile,
    SendProgress,
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
            // The last reader of a session Core is not running: its view
            // goes, and is projected afresh when it is opened again.
            handle.close_if_idle();
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
    use super::*;

    fn subscriber() -> Subscriber {
        Subscriber {
            events: mpsc::channel(1).0,
            overflowed: Arc::default(),
            kick: Arc::new(watch::Sender::new(false)),
        }
    }

    #[tokio::test]
    async fn a_read_still_running_is_waited_on_never_started_beside_it() {
        use std::sync::atomic::AtomicUsize;
        let slot: ReadSlot<u32> = ReadSlot::default();
        let starts = AtomicUsize::new(0);
        let gate = Arc::new(Notify::new());
        let slow = || {
            starts.fetch_add(1, Ordering::SeqCst);
            let gate = Arc::clone(&gate);
            tokio::spawn(async move {
                gate.notified().await;
                7
            })
        };
        let short = Duration::from_millis(20);
        assert_eq!(read_once(&slot, short, slow, async |_| false).await, None);
        // The disk still hangs: the next open waits on the same read.
        assert_eq!(read_once(&slot, short, slow, async |_| false).await, None);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        gate.notify_one();
        // Joining it, this open reads again once it finishes: the record may
        // have grown meanwhile.
        let current = || {
            starts.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async { 9 })
        };
        assert_eq!(
            read_once(&slot, Duration::from_secs(5), current, async |_| false).await,
            Some(9)
        );
        assert_eq!(starts.load(Ordering::SeqCst), 2);

        // A read that finished after its caller gave up is not handed on:
        // the record may have changed since. A fresh one is started.
        let gate = Arc::new(Notify::new());
        let late = || {
            starts.fetch_add(1, Ordering::SeqCst);
            let gate = Arc::clone(&gate);
            tokio::spawn(async move {
                gate.notified().await;
                1
            })
        };
        assert_eq!(read_once(&slot, short, late, async |_| false).await, None);
        gate.notify_one();
        tokio::time::sleep(short).await;
        let fresh = || {
            starts.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async { 2 })
        };
        assert_eq!(
            read_once(&slot, Duration::from_secs(5), fresh, async |_| false).await,
            Some(2)
        );
        assert_eq!(starts.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn a_joined_read_is_taken_while_the_record_is_unchanged() {
        use std::sync::atomic::AtomicUsize;
        let slot: ReadSlot<u32> = ReadSlot::default();
        let starts = AtomicUsize::new(0);
        let gate = Arc::new(Notify::new());
        let slow = || {
            starts.fetch_add(1, Ordering::SeqCst);
            let gate = Arc::clone(&gate);
            tokio::spawn(async move {
                gate.notified().await;
                7
            })
        };
        let short = Duration::from_millis(20);
        assert_eq!(read_once(&slot, short, slow, async |_| true).await, None);
        gate.notify_one();
        // Unchanged: the read already made is taken, none started beside it,
        // so a record slower to read than the timeout still opens.
        assert_eq!(
            read_once(&slot, Duration::from_secs(5), slow, async |read| *read == 7).await,
            Some(7)
        );
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    // Paused time on one thread: a sleep lets every other task run until it
    // waits, and no timeout runs out early on a busy machine.
    #[tokio::test(start_paused = true)]
    async fn a_joined_read_whose_record_cannot_be_checked_fails_the_open_and_holds_nothing() {
        let slot: Arc<ReadSlot<u32>> = Arc::default();
        let gate = Arc::new(Notify::new());
        let slow = || {
            let gate = Arc::clone(&gate);
            tokio::spawn(async move {
                gate.notified().await;
                7
            })
        };
        let short = Duration::from_millis(20);
        assert_eq!(read_once(&slot, short, slow, async |_| true).await, None);
        // An open joins the read still running...
        let stuck = tokio::spawn({
            let slot = Arc::clone(&slot);
            async move {
                read_once(
                    &slot,
                    Duration::from_millis(500),
                    || tokio::spawn(async { 0 }),
                    async |_| std::future::pending::<bool>().await,
                )
                .await
            }
        });
        tokio::time::sleep(short).await;
        // ...which finishes, then the disk stops answering: checking whether
        // the record changed never ends.
        gate.notify_one();
        tokio::time::sleep(short).await;
        // Meanwhile another open is not held behind that check.
        let other = read_once(&slot, short, || tokio::spawn(async { 9 }), async |_| true);
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(100), other).await,
            Ok(Some(9))
        );
        // And the stuck one gives up as a read that takes too long does.
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), stuck)
                .await
                .unwrap()
                .unwrap(),
            None
        );
    }

    /// An adapter whose record and summary reads wait on gates, as reads on
    /// a disk that stopped answering do, and which counts its forgets. While
    /// `stats_hang` is set, taking the record's version never returns, as a
    /// stat on such a disk does; `hung_stats` counts those calls, each of
    /// which would hold a blocking thread for ever.
    #[derive(Default)]
    struct GatedReads {
        record: Notify,
        summary: Notify,
        forgets: std::sync::atomic::AtomicUsize,
        stats_hang: AtomicBool,
        hung_stats: std::sync::atomic::AtomicUsize,
    }

    #[ersa_adapter::async_trait]
    impl Adapter for GatedReads {
        fn backend(&self) -> &str {
            "gated"
        }
        async fn start(
            &self,
            _transport: Arc<dyn Transport>,
            _config: SessionConfig,
            _sink: Arc<dyn AdapterSink>,
        ) -> Result<Box<dyn AdapterSession>, AdapterError> {
            Err(AdapterError::Protocol("never started".into()))
        }
        fn projector(&self) -> Box<dyn ersa_adapter::Projector> {
            unreachable!("never started")
        }
        async fn read_record(
            &self,
            _transport: Arc<dyn Transport>,
            _cwd: &str,
            _backend_session_id: &str,
        ) -> Result<Arc<Vec<RecordEvent>>, AdapterError> {
            self.record.notified().await;
            Ok(Arc::default())
        }
        async fn summarize_record(
            &self,
            _transport: Arc<dyn Transport>,
            _cwd: &str,
            _backend_session_id: &str,
        ) -> Result<Option<RecordSummary>, AdapterError> {
            self.summary.notified().await;
            Ok(None)
        }
        async fn record_version(
            &self,
            _transport: Arc<dyn Transport>,
            _cwd: &str,
            _backend_session_id: &str,
        ) -> Result<Option<String>, AdapterError> {
            if self.stats_hang.load(Ordering::SeqCst) {
                self.hung_stats.fetch_add(1, Ordering::SeqCst);
                std::future::pending::<()>().await;
            }
            Ok(None)
        }
        fn forget_record(&self, _cwd: &str, _backend_session_id: &str) {
            self.forgets.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// A manager with one session, `s-gated`, of the [`GatedReads`]
    /// adapter, which it never started.
    fn gated_manager(dir: &std::path::Path, adapter: &Arc<GatedReads>) -> SessionManager {
        let store = Store::open(&dir.join("ersa.db")).unwrap();
        let (app_log, _) = AppLog::open(dir).unwrap();
        let manager = SessionManager::new(
            store,
            vec![Arc::clone(adapter) as Arc<dyn Adapter>],
            Arc::new(app_log),
        );
        let row = SessionRow {
            id: "s-gated".into(),
            workspace_id: "w".into(),
            host_id: "local".into(),
            backend: "gated".into(),
            backend_session_id: Some("b-gated".into()),
            mode: None,
            cwd: "C:/work".into(),
            model: None,
            effort: None,
            permission_mode: None,
            archived: false,
            deleting: None,
            pinned: false,
            section_id: None,
            last_activity_at: None,
        };
        let handle = Arc::new(Handle::new(row, now_rfc3339()));
        lock(&manager.sessions).insert(handle.id.clone(), handle);
        manager
    }

    /// Opens `s-gated`'s view from the start, as a client that subscribes
    /// with nobody else reading does, and lets the subscription go.
    async fn open_gated(manager: &SessionManager) {
        let transport: Arc<dyn Transport> = Arc::new(ersa_adapter::LocalTransport);
        let opened = manager
            .subscribe(
                "s-gated",
                0,
                |_| Some(Arc::clone(&transport)),
                mpsc::channel(16).0,
                Arc::new(watch::Sender::new(false)),
            )
            .await;
        assert!(opened.is_ok());
    }

    // Paused time: the reads' timeouts run out as soon as nothing else can.
    #[tokio::test(start_paused = true)]
    async fn opens_on_a_disk_whose_stats_hang_leave_one_version_check_not_one_each() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = Arc::new(GatedReads::default());
        let manager = Arc::new(gated_manager(dir.path(), &adapter));
        for _ in 0..3 {
            adapter.stats_hang.store(false, Ordering::SeqCst);
            // An open starts a read of the record, which outlasts it.
            open_gated(&manager).await;
            // The next open joins that read...
            let joiner = tokio::spawn({
                let manager = Arc::clone(&manager);
                async move { open_gated(&manager).await }
            });
            tokio::time::sleep(Duration::from_millis(1)).await;
            // ...which finishes; then the disk stops answering stats, so
            // checking whether the record changed since never ends.
            adapter.stats_hang.store(true, Ordering::SeqCst);
            adapter.record.notify_one();
            joiner.await.unwrap();
        }
        // Every later open joined the check still running.
        assert_eq!(adapter.hung_stats.load(Ordering::SeqCst), 1);
    }

    // Paused time: the reads' timeouts run out as soon as nothing else can.
    #[tokio::test(start_paused = true)]
    async fn reads_that_outlive_a_removed_session_have_the_adapter_forget_its_record() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = Arc::new(GatedReads::default());
        let manager = gated_manager(dir.path(), &adapter);

        // Opening it: the record read and then the summary read time out and
        // are left running.
        open_gated(&manager).await;

        manager.remove("s-gated").await.unwrap();
        assert_eq!(adapter.forgets.load(Ordering::SeqCst), 1);
        // Each read that ends after the removal forgets what it kept.
        adapter.record.notify_one();
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert_eq!(adapter.forgets.load(Ordering::SeqCst), 2);
        adapter.summary.notify_one();
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert_eq!(adapter.forgets.load(Ordering::SeqCst), 3);
    }

    /// Makes `s-gated` live by hand, as the session task does, and returns
    /// its sink and the sender that marks it finished.
    fn make_live(manager: &SessionManager) -> (Arc<CoreSink>, watch::Sender<bool>) {
        let handle = manager.handle("s-gated").unwrap();
        let sink = Arc::new(CoreSink::new(
            "s-gated",
            Arc::clone(&manager.app_log),
            mpsc::unbounded_channel().0,
        ));
        let finished = watch::Sender::new(false);
        let live = Live {
            sink: Arc::clone(&sink),
            started: watch::channel(Started::Pending).1,
            cancel: watch::Sender::new(false),
            progress: watch::channel(Progress::default()).1,
            finished: finished.subscribe(),
        };
        *lock(&handle.live) = Some(Arc::new(live));
        (sink, finished)
    }

    // verifies: PROTO-CORE-API-R41
    // Paused time: the wind-down below takes as long as the test says.
    #[tokio::test(start_paused = true)]
    async fn a_session_whose_end_was_announced_is_removed_once_it_winds_down() {
        let adapter = Arc::new(GatedReads::default());
        // The two ways a session ends: its process exits, or its start fails.
        let endings: [fn(&CoreSink); 2] = [
            |sink| {
                sink.start_finished(Ok(()));
                sink.process_exited(Ok(ProcessExit { code: Some(0) }));
            },
            |sink| {
                sink.start_finished(Err(StartFailure {
                    message: "no".into(),
                    stopped_by_user: false,
                }))
            },
        ];
        for end in endings {
            let dir = tempfile::tempdir().unwrap();
            let manager = gated_manager(dir.path(), &adapter);
            let (sink, finished) = make_live(&manager);
            // Running and not ended: refused.
            assert!(matches!(
                manager.remove("s-gated").await,
                Err(SessionError::InvalidState(_))
            ));
            // Its end is queued, so a client may have seen `session.ended`,
            // while Core still holds it live for a while.
            end(&sink);
            let handle = manager.handle("s-gated").unwrap();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                *lock(&handle.live) = None;
                finished.send_replace(true);
            });
            manager.remove("s-gated").await.unwrap();
        }
    }

    // verifies: PROTO-CORE-API-R41
    // Paused time: the wait runs out as soon as nothing else can happen.
    #[tokio::test(start_paused = true)]
    async fn an_ended_session_that_never_winds_down_is_refused_without_holding_it() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = Arc::new(GatedReads::default());
        let manager = Arc::new(gated_manager(dir.path(), &adapter));
        // Stored too, in a workspace of its own, so a removal shows there.
        let store = &manager.store;
        store.ensure_local_host("pc".into()).await.unwrap();
        let project = ersa_protocol::core_api::Project {
            id: "p".into(),
            host_id: persistence::LOCAL_HOST_ID.into(),
            path: "C:/work".into(),
            name: "work".into(),
            vcs: ersa_protocol::core_api::Vcs::Git,
            trusted: false,
        };
        let workspace = store
            .insert_project(project, "c:/work".into())
            .await
            .unwrap();
        let mut row = lock(&manager.handle("s-gated").unwrap().row).clone();
        row.workspace_id = workspace.id;
        store.insert_session(row).await.unwrap();
        let (sink, _finished) = make_live(&manager);
        sink.start_finished(Ok(()));
        sink.process_exited(Ok(ProcessExit { code: Some(0) }));
        let removing = tokio::spawn({
            let manager = Arc::clone(&manager);
            async move { manager.remove("s-gated").await }
        });
        tokio::time::sleep(Duration::from_millis(1)).await;
        // While it waits, the session's other operations are not held up.
        let handle = manager.handle("s-gated").unwrap();
        assert!(handle.op.try_lock().is_ok());
        // Core's own wind-down is what stalled, and the error says so.
        match removing.await.unwrap() {
            Err(SessionError::Internal(message)) => {
                assert!(message.contains("has ended"), "{message}");
            }
            other => panic!("expected an internal error, got {other:?}"),
        }
        // Giving up removed nothing: the session, its row and what the
        // adapter keeps of its record are all still there.
        assert!(manager.handle("s-gated").is_ok());
        assert!(
            manager
                .store
                .session("s-gated".into())
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(adapter.forgets.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_read_that_ends_after_its_session_is_removed_lets_go_of_what_it_kept() {
        let removed = AtomicBool::new(false);
        let forgot = AtomicBool::new(false);
        let forget = || forgot.store(true, Ordering::SeqCst);
        assert_eq!(read_unless_removed(async { 1 }, &removed, forget).await, 1);
        assert!(!forgot.load(Ordering::SeqCst));
        // The session is removed while the read runs: what it kept goes.
        let read = async {
            removed.store(true, Ordering::SeqCst);
            2
        };
        assert_eq!(read_unless_removed(read, &removed, forget).await, 2);
        assert!(forgot.load(Ordering::SeqCst));
    }

    #[test]
    fn a_read_from_before_its_record_existed_does_not_hold_once_it_does() {
        let empty: RecordReadResult = Ok((None, Arc::default()));
        assert!(still_holds(&empty, None));
        assert!(!still_holds(&empty, Some("10:1")));
        let read: RecordReadResult = Ok((Some("10:1".into()), Arc::default()));
        assert!(still_holds(&read, Some("10:1")));
        assert!(!still_holds(&read, Some("20:2")));
        assert!(!still_holds(&read, None));
        // A read that failed is reported as it is.
        assert!(still_holds(&Err("denied".into()), Some("10:1")));
    }

    #[test]
    fn a_view_holds_its_latest_events_and_a_subscribe_from_before_them_starts_there() {
        let mut hub = Hub::default();
        for seq in 0..10 {
            let line = format!("event {seq:02}"); // 8 bytes each
            hub.hold_within(Arc::new(Published { seq, line }), 32);
        }
        assert_eq!(hub.first_seq, 6);
        assert_eq!(hub.last_seq(), Some(9));
        let (_, subscribed, backlog) = register(&mut hub, subscriber(), 2);
        assert_eq!(subscribed.first_seq, Some(6));
        assert_eq!(subscribed.last_seq, Some(9));
        let seqs: Vec<u64> = backlog.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, [6, 7, 8, 9]);
        // From within what is held, as asked.
        let (_, _, backlog) = register(&mut hub, subscriber(), 8);
        assert_eq!(backlog.iter().map(|e| e.seq).collect::<Vec<_>>(), [8, 9]);
    }

    #[test]
    fn only_rfc3339_times_are_taken_from_a_record() {
        for good in ["2026-11-01T20:56:51.893Z", "2026-11-01T20:56:51Z"] {
            assert!(is_rfc3339(good), "{good}");
        }
        for bad in [
            "",
            "yesterday",
            "2026-11-01",
            "2026-11-01T20:56:51",
            "2026-11-01T20:56:51.Z",
            // Right, but would not sort among the times Core issues.
            "2026-11-01T20:56:51+02:00",
        ] {
            assert!(!is_rfc3339(bad), "{bad}");
        }
    }
}
