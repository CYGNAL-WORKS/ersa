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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, BackendChoices, CallOff, CallOffOnDrop,
    Direction, OpenElsewhere, PermissionDecision, ProcessExit, QuestionReply, RawLine,
    RecordDeletion, RecordEvent, RecordSummary, SessionConfig, TitleRules, Transport,
};
use ersa_protocol::core_api::rpc::Notification;
use ersa_protocol::core_api::{
    Deleting, ErrorReason, Section, Session, SessionEventParams, SessionTileParams, Tile,
};
use ersa_protocol::events::{
    Envelope, ErrorCode, ErrorEvent, Event, LifecycleState, NoticeCode, NoticeLevel, SessionNotice,
};
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::task::AbortHandle;

use crate::admission::{Admission, Command, Refusal, Report, RowChange, UnderWay};
use crate::app_log::{AppLog, Facts, LifecycleFact};
use crate::persistence::{self, SessionRow, Store, now_rfc3339};
use crate::projection::View;
use crate::raw_log::RawLog;

mod fork;
mod found;
mod resume;
mod send;
mod settings;

pub use fork::{Beginning, ForkHold, Forkable};
pub use resume::Resumption;
pub use send::{Pacer, Waiting};

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
/// may take. A registry that hangs makes the answer unknown rather than
/// holding up what was asked for.
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
    /// A refusal the user meets: answered `invalid_state` with `reason`, so
    /// the Interface can say it in its own words; `message` is for the log
    /// (PROTO-CORE-API, Errors).
    #[error("{message}")]
    Refused {
        reason: ErrorReason,
        message: String,
        /// What the reason names, when it names something: the processes
        /// for `open_elsewhere`.
        detail: Option<String>,
    },
}

impl SessionError {
    /// A refusal the user meets, for `reason`, with `message` for the log.
    pub fn refused(reason: ErrorReason, message: impl Into<String>) -> Self {
        SessionError::Refused {
            reason,
            message: message.into(),
            detail: None,
        }
    }

    /// The refusal for a session other processes have open: `detail` lists
    /// their pids, so the Interface can name them in its own words.
    pub fn open_elsewhere(pids: &[u32], message: impl Into<String>) -> Self {
        SessionError::Refused {
            reason: ErrorReason::OpenElsewhere,
            message: message.into(),
            detail: Some(pid_list(pids)),
        }
    }
}

/// Every session Core knows, and the adapters it can start them with.
pub struct SessionManager {
    store: Store,
    /// The registered adapters, in the order they were registered, which
    /// is the order backends are listed in (PROTO-CORE-API-R65). Few enough
    /// to look up by walking the list.
    adapters: Vec<Arc<dyn Adapter>>,
    /// Where Core writes each backend process's lifecycle facts.
    app_log: Arc<AppLog>,
    /// Decides whether a workspace takes a session, and is told which
    /// sessions exist (ARCH-CORE, Admission).
    admission: Arc<Admission>,
    /// Every session in the database, by id. A std mutex: only held to look
    /// up, insert, or remove, never across an await.
    sessions: Mutex<HashMap<String, Arc<Handle>>>,
    /// Where `session.tile` notifications go, for every attached client.
    tiles: Arc<TileHub>,
    /// A pause after each chunk an attachment's copy writes, for tests.
    copy_pause: Duration,
    /// A pause before Core lets go of a run that has ended, for tests.
    wind_down_pause: Duration,
    /// Where a list stops between storing a found session and holding it,
    /// for tests ([`crate::CoreConfig::found_gate`]).
    found_gate: Option<Arc<crate::state::Gate>>,
    /// Held while Core lists the sessions found in the backends' records,
    /// and while a delete removes a session's row, so a list never records
    /// a session twice or finds a deleted one again (`found`).
    listing: tokio::sync::Mutex<()>,
    /// The folders whose listing is still under way (`found`).
    listing_in_flight: found::InFlight,
    /// How many deletes have removed a session's row, counted under the
    /// listing turn (`found`).
    deletes: AtomicU64,
}

/// Whether the session is starting, a turn is running in it (one waiting
/// on the user included), or a message is being sent to it: what Delete and
/// Archive wait for (PROTO-CORE-API, `session_busy`).
fn in_turn(handle: &Handle) -> bool {
    // One that is gone has no turn.
    let Some(session) = handle.describe() else {
        return false;
    };
    let working = matches!(
        session.state,
        LifecycleState::Starting | LifecycleState::Running | LifecycleState::WaitingUser
    );
    // A pending permission or question is a turn waiting on the user,
    // whatever state the backend last reported.
    let waiting = !session.tile.pending.is_empty();
    (handle.live().is_some() && (working || waiting)) || session.sending.is_some()
}

/// The answer for a command about session `session_id`, in workspace
/// `workspace_id`, that admission refused, worded as before admission
/// decided it.
fn refused_session(refusal: Refusal, workspace_id: &str, session_id: &str) -> SessionError {
    match refusal {
        Refusal::WorkspaceHeld => busy(workspace_id),
        Refusal::NoWorkspace | Refusal::WorkspaceNotReady => {
            SessionError::InvalidState(format!("workspace {workspace_id} is not ready"))
        }
        Refusal::NoSession => SessionError::NotFound(session_id.to_string()),
        Refusal::Closing => {
            SessionError::InvalidState("Core is shutting down; no session can start".into())
        }
        Refusal::Running => {
            SessionError::InvalidState(format!("session {session_id} is running; stop it first"))
        }
        Refusal::BeingArchived => send::being_archived(),
        // The run was announced ended, and Core's own wind-down stalled.
        Refusal::GaveUp => SessionError::Internal(format!(
            "session {session_id} has ended, but Core has not finished with it within {STOP_TIMEOUT:?}"
        )),
        // Answers to other commands, or an id used twice: Core's fault.
        Refusal::AlreadyProject { .. }
        | Refusal::NoProject
        | Refusal::ProjectBusy { .. }
        | Refusal::ProjectHasSessions
        | Refusal::ProjectHasWorkspaces
        | Refusal::WorkspaceExists
        | Refusal::RootWorkspace
        | Refusal::WorkspaceBusy
        | Refusal::WorkspaceHasSessions
        | Refusal::NoSection
        | Refusal::SessionExists => SessionError::Internal(format!(
            "admission refused session {session_id} in workspace {workspace_id}: {refusal:?}"
        )),
    }
}

/// Stops `handle`'s backend and waits until it has ended and every line is
/// projected, for a caller that holds the session's exclusive operation.
/// One that is not running is left as it is.
async fn stop_held(handle: &Handle) -> Result<(), SessionError> {
    let Some(live) = handle.live() else {
        return Ok(());
    };
    begin_stop(&live);
    wait_finished(&live, STOP_TIMEOUT).await
}

/// The refusal for a request a delete under way holds back.
pub(crate) fn busy(workspace_id: &str) -> SessionError {
    SessionError::Busy(format!(
        "workspace {workspace_id} is busy: a session in it is being deleted, or the workspace removed; try again once it is done"
    ))
}

/// One session, live or not.
struct Handle {
    /// Raised from when a fork of this session starts reading its record
    /// until the fork's backend is ready. While it is, at most one such read
    /// is stuck on a disk that hangs, however often the user tries again, and
    /// `session.send` refuses: the CLI has yet to copy the record.
    forking: Arc<AtomicBool>,
    id: String,
    backend: String,
    /// The workspace the session is in, which never changes.
    workspace_id: String,
    /// Keeps the session's row ([`Handle::row`]).
    admission: Arc<Admission>,
    /// The backend's default permission mode, for a session whose row has
    /// none: one from before Core recorded it, or one found in the
    /// backend's records, was started stating that default.
    permission_default: Option<String>,
    /// What the session's record says of it, as last read.
    presented: Mutex<Presented>,
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
    /// The same for looking at the session's folder before a resume: on a
    /// network drive that stops answering, one stuck look per session.
    folder_check: ReadSlot<resume::FolderLook>,
    /// The send under way, if any: one at a time per session.
    sending: Mutex<Option<send::Sending>>,
    /// Woken when a send ends, so a send waiting its turn behind it reports
    /// at once (PROTO-CORE-API-R90).
    send_ended: Arc<Notify>,
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
/// `remove` tells admission the session is gone before it forgets, so
/// either its forget comes after anything this read kept, or this finds the
/// session gone (`gone` says whether it is).
async fn read_unless_removed<T>(
    read: impl Future<Output = T>,
    gone: impl FnOnce() -> bool,
    forget: impl FnOnce(),
) -> T {
    let value = read.await;
    if gone() {
        forget();
    }
    value
}

/// One look at a session's files (its record, or its folder) that may
/// still be running.
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
                    tracing::error!(error = %e, "a look at a session's files failed");
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
    /// The record's title, a rename Core sent, or the latest title the
    /// backend announced on its live stream, whichever came last: until
    /// the record holds a title, the announced one is the session's
    /// (PROTO-CLAUDE-CODE-STREAM-R36), kept in memory only.
    title: Option<String>,
    /// RFC 3339: from the record when it has said, else when Core first
    /// started the session's backend, else [`UNKNOWN_TIME`].
    created_at: String,
    /// What the record last said of the session's model, effort and
    /// permission mode, and what its live stream last reported of them
    /// (PROTO-CORE-API-R71).
    recorded: ersa_adapter::RecordedSettings,
    reported: settings::Reported,
}

impl Presented {
    /// Takes what the session's record says, and answers whether the title
    /// or the date created changed. Every tile carries both as presented, so
    /// the ones presented until now are the ones the last tile carried, and
    /// a change is sent to every window (PROTO-CORE-API-R103, R106).
    fn take_summary(&mut self, summary: RecordSummary, live: bool, session: &str) -> bool {
        let retitled = summary.title.is_some() && summary.title != self.title;
        if summary.title.is_some() {
            self.title = summary.title;
        }
        self.recorded = summary.settings;
        // A session not running has no stream: the record holds what it
        // last reported (PROTO-CORE-API-R71).
        if !live {
            self.reported = settings::Reported::default();
        }
        // The record's wrapper is the backend's and changes between
        // versions: a time that is not RFC 3339 is not used.
        let mut redated = false;
        match summary.created_at {
            Some(at) if is_rfc3339(&at) => {
                redated = at != self.created_at;
                self.created_at = at;
            }
            Some(at) => {
                tracing::warn!(session, created_at = %at, "the session record's first time is not RFC 3339; not used");
            }
            None => {}
        }
        retitled || redated
    }
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
    /// The backend holds messages behind its running turn, which are not
    /// projected until it takes them in (PROTO-CORE-API-R81).
    holding: bool,
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

/// The refusal for a request that needs a backend the session does not have.
pub(crate) fn not_running() -> SessionError {
    SessionError::refused(ErrorReason::SessionNotRunning, "the session is not running")
}

impl Handle {
    /// The session with row `row`, which `admission` keeps, first known to
    /// have begun at `created_at`, until its record says.
    fn new(
        row: &SessionRow,
        created_at: String,
        admission: Arc<Admission>,
        permission_default: Option<String>,
    ) -> Self {
        Handle {
            forking: Arc::default(),
            id: row.id.clone(),
            backend: row.backend.clone(),
            workspace_id: row.workspace_id.clone(),
            admission,
            permission_default,
            presented: Mutex::new(Presented {
                title: None,
                created_at,
                recorded: Default::default(),
                reported: Default::default(),
            }),
            hub: Mutex::default(),
            live: Mutex::new(None),
            raw: Mutex::default(),
            ending: Mutex::new(None),
            start_failure: Mutex::new(None),
            record_read: tokio::sync::Mutex::new(None),
            version_check: tokio::sync::Mutex::new(None),
            summary_read: tokio::sync::Mutex::new(None),
            folder_check: tokio::sync::Mutex::new(None),
            sending: Mutex::default(),
            send_ended: Arc::default(),
            scratch: Arc::default(),
        }
    }

    fn live(&self) -> Option<Arc<Live>> {
        lock(&self.live).clone()
    }

    /// The error for a request that needs the backend running when it is
    /// not: the reason it failed to start, when it did. That one carries no
    /// `reason`, so the Interface shows the backend's own words (not being
    /// signed in, say) rather than its words for a stopped session
    /// (FLOW-NEW-SESSION-R08).
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

    /// The session's row as last stored, which admission keeps
    /// (ARCH-CORE-R14); `None` once the session is gone.
    fn stored_row(&self) -> Option<SessionRow> {
        let mut row = self.admission.row(&self.id)?;
        if row.permission_mode.is_none() {
            row.permission_mode = self.permission_default.clone();
        }
        Some(row)
    }

    /// The session's row; `not_found` once the session is gone.
    fn row(&self) -> Result<SessionRow, SessionError> {
        self.stored_row()
            .ok_or_else(|| SessionError::NotFound(self.id.clone()))
    }

    /// Asks, when called, whether the session is gone by then: admission
    /// no longer has it. For a read of its record that may outlive it.
    fn when_gone(&self) -> impl FnOnce() -> bool + Send + 'static {
        let (admission, id) = (Arc::clone(&self.admission), self.id.clone());
        move || !admission.has_session(&id)
    }

    /// The session's state and tile, as its view has them. A session no
    /// view has been opened for is not running in this Core, so it has
    /// ended: as its ending left it, which a view would end with too (an
    /// exit with an error is `error`).
    fn state_and_tile(&self) -> (LifecycleState, Tile) {
        let ending = lock(&self.ending).clone();
        let hub = lock(&self.hub);
        self.state_and_tile_in(&hub, &ending)
    }

    fn state_and_tile_in(
        &self,
        hub: &Hub,
        ending: &Option<(LifecycleFact, String)>,
    ) -> (LifecycleState, Tile) {
        match &hub.view {
            Some(view) => (view.state(), view.tile().clone()),
            None => {
                let (mut view, _) = View::open(&self.id, &self.backend, None, &[]);
                if let Some((fact, ts)) = ending {
                    view.lifecycle(fact, ts);
                }
                (view.state(), view.tile().clone())
            }
        }
    }

    /// The session as `session.list` reports it; `None` once it is gone.
    fn describe(&self) -> Option<Session> {
        self.stored_row().map(|row| self.describe_from(row))
    }

    /// The session as it is; `not_found` once it is gone.
    fn session(&self) -> Result<Session, SessionError> {
        self.row().map(|row| self.describe_from(row))
    }

    /// The session with row `row`, as `session.list` reports it.
    fn describe_from(&self, row: SessionRow) -> Session {
        let presented = lock(&self.presented).clone();
        let ending = lock(&self.ending).clone();
        let hub = lock(&self.hub);
        let (state, tile) = self.state_and_tile_in(&hub, &ending);
        let listed = settings::listed(&row, &presented.recorded, &presented.reported);
        Session {
            id: row.id,
            // Filled by the tickets that build them (PROTO-CORE-API,
            // Sessions): the backend's id and the stored unseen mark.
            backend_session_id: None,
            workspace_id: row.workspace_id,
            host_id: row.host_id,
            backend: row.backend,
            model: listed.model,
            // Core's own request beside the backend's report (PROTO-CORE-API-R102).
            model_request: row.model.clone(),
            effort: listed.effort,
            permission_mode: listed.permission_mode.unwrap_or_default(),
            mode: row.mode,
            title: presented.title,
            state,
            created_at: presented.created_at,
            last_event_seq: hub.last_seq(),
            sending: lock(&self.sending).as_ref().map(send::Sending::phase),
            archived: row.archived,
            pinned: row.pinned,
            section_id: row.section_id,
            unseen: false,
            last_activity_at: row.last_activity_at,
            deleting: row.deleting,
            tile,
        }
    }
}

impl SessionManager {
    /// A manager over `store`, starting sessions with `adapters`, asking
    /// `admission` whether a workspace takes a session and telling it which
    /// sessions exist.
    pub fn new(
        store: Store,
        adapters: Vec<Arc<dyn Adapter>>,
        app_log: Arc<AppLog>,
        admission: Arc<Admission>,
    ) -> Self {
        // One adapter per backend: a second registration of a name is a
        // mistake in the build, and the first one stands.
        let mut registered: Vec<Arc<dyn Adapter>> = Vec::new();
        for adapter in adapters {
            if registered.iter().any(|a| a.backend() == adapter.backend()) {
                tracing::error!(
                    backend = adapter.backend(),
                    "an adapter for this backend is registered twice; the first stands"
                );
            } else {
                registered.push(adapter);
            }
        }
        SessionManager {
            store,
            adapters: registered,
            app_log,
            admission,
            sessions: Mutex::new(HashMap::new()),
            tiles: Arc::default(),
            copy_pause: Duration::ZERO,
            wind_down_pause: Duration::ZERO,
            found_gate: None,
            listing: tokio::sync::Mutex::new(()),
            listing_in_flight: Arc::default(),
            deletes: AtomicU64::new(0),
        }
    }

    /// The same manager, pausing after each chunk an attachment's copy
    /// writes ([`crate::CoreConfig::copy_pause`]).
    pub fn with_copy_pause(mut self, copy_pause: Duration) -> Self {
        self.copy_pause = copy_pause;
        self
    }

    /// The same manager, pausing before it lets go of a run that has ended
    /// ([`crate::CoreConfig::wind_down_pause`]).
    pub fn with_wind_down_pause(mut self, pause: Duration) -> Self {
        self.wind_down_pause = pause;
        self
    }

    /// The same manager, stopping a list between storing a found session
    /// and holding it ([`crate::CoreConfig::found_gate`]).
    pub fn with_found_gate(mut self, gate: Option<Arc<crate::state::Gate>>) -> Self {
        self.found_gate = gate;
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
        // Every stored session counts here, dormant ones included, so their
        // facts are kept for when their adapter is back.
        self.app_log
            .retain_only(rows.iter().map(|row| row.id.clone()).collect());
        for row in rows {
            let known = facts.get(&row.id);
            // How the session last ended. One a previous Core left running
            // ended with that Core: said now, whether or not its adapter is
            // registered, so a dormant session's end is not dated to
            // whenever its adapter is back.
            let ending = match known.map(|f| &f.last) {
                Some(Some(last)) if !last.fact.ends() => {
                    let ts = now_rfc3339();
                    let fact = LifecycleFact::CoreRestarted;
                    self.app_log.write(&row.id, &ts, &fact);
                    Some((fact, ts))
                }
                Some(Some(last)) => Some((last.fact.clone(), last.ts.clone())),
                // Its last fact could not be read: how it ended is unknown,
                // and nothing is made up for it.
                Some(None) | None => None,
            };
            // A session whose backend has no registered adapter is dormant
            // (PROTO-CORE-API-R66, R67): not loaded, so absent from every
            // list and `not_found` by id, while its row and its facts stay
            // stored for a Core that has the adapter again. Every session
            // loaded therefore has an adapter.
            let Some(adapter) = self.adapter(&row.backend) else {
                continue;
            };
            // Sessions from before Core recorded the permission mode were
            // all started stating their backend's default.
            let default = BackendChoices::default_of(&adapter.choices().permission_modes);
            let default = (!default.is_empty()).then_some(default);
            let started = known.map_or(UNKNOWN_TIME.to_string(), |f| f.first.ts.clone());
            let admission = Arc::clone(&self.admission);
            let handle = Arc::new(Handle::new(&row, started, admission, default));
            *lock(&handle.ending) = ending;
            // As admission keeps it, with that default.
            let Some(row) = handle.stored_row() else {
                continue;
            };
            if let (true, Some(transport)) = (
                row.backend_session_id.is_some(),
                transport_for(&row.host_id),
            ) {
                let handle = Arc::clone(&handle);
                let store = self.store.clone();
                let tiles = self.tiles.clone();
                summaries.spawn(async move {
                    read_summary(&store, &handle, &tiles, adapter, transport, row).await;
                });
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

    /// The adapter registered for `backend`.
    pub fn adapter(&self, backend: &str) -> Option<Arc<dyn Adapter>> {
        self.adapters
            .iter()
            .find(|adapter| adapter.backend() == backend)
            .cloned()
    }

    /// Every registered adapter, in the order they were registered.
    pub fn adapters(&self) -> &[Arc<dyn Adapter>] {
        &self.adapters
    }

    /// The adapter of a session Core has loaded, or is creating. Every
    /// loaded session has one, since a session without one stays dormant
    /// (see [`SessionManager::recover`]); a session created for a backend
    /// with none is refused here.
    pub fn adapter_of(&self, backend: &str) -> Result<Arc<dyn Adapter>, SessionError> {
        self.adapter(backend).ok_or_else(|| {
            SessionError::BackendMissing(format!("no adapter for backend {backend}"))
        })
    }

    /// The titles session `id`'s backend takes, as its adapter states them
    /// (PROTO-CORE-API-R70).
    pub fn title_rules(&self, id: &str) -> Result<TitleRules, SessionError> {
        let handle = self.handle(id)?;
        Ok(self.adapter_of(&handle.backend)?.title_rules())
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
            .iter()
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

    /// Every session, oldest first.
    pub fn list(&self) -> Vec<Session> {
        let handles: Vec<Arc<Handle>> = lock(&self.sessions).values().cloned().collect();
        // One that is gone, and not let go of yet, is not listed.
        let mut sessions: Vec<Session> = handles.iter().filter_map(|h| h.describe()).collect();
        sessions.sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
        sessions
    }

    /// One session.
    pub fn get(&self, id: &str) -> Result<Session, SessionError> {
        self.handle(id)?.session()
    }

    /// How many sessions are running, and how many are waiting on the user.
    pub fn counts(&self) -> (u64, u64) {
        let handles: Vec<Arc<Handle>> = lock(&self.sessions).values().cloned().collect();
        let (mut running, mut waiting) = (0, 0);
        for handle in handles.iter().filter(|h| h.live().is_some()) {
            let Some(session) = handle.describe() else {
                continue;
            };
            match session.state {
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
        let adapter = self.adapter_of(&row.backend)?;
        self.create_with(
            row,
            Beginning::default(),
            adapter,
            transport,
            config,
            backend_version,
        )
        .await
    }

    /// [`SessionManager::create`] with a given adapter rather than the
    /// registered one for the session's backend, and what the session
    /// `begin`s with: the replay server's, which feeds a recording in place
    /// of a backend, and a fork's.
    pub async fn create_with(
        &self,
        row: SessionRow,
        begin: Beginning,
        adapter: Arc<dyn Adapter>,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        backend_version: String,
    ) -> Result<Session, SessionError> {
        let (workspace_id, id) = (row.workspace_id.clone(), row.id.clone());
        // Admission decides whether the workspace takes a session, in the
        // step that also counts it there: a removal admitted before this
        // refuses it, and one asked for after sees it (ARCH-CORE-R14). None
        // is created once Core is shutting down.
        let admitted = self
            .admission
            .command(Command::CreateSession {
                workspace: row.workspace_id.clone(),
                session: row.id.clone(),
                found: false,
            })
            .await
            .map_err(|refusal| refused_session(refusal, &row.workspace_id, &row.id))?;
        // Only what was given the backend is stored, with when (PROTO-CORE-API-R71).
        self.store.insert_session(row.clone()).await?;
        admitted.end(Report::SessionStored {
            row: Box::new(row.clone()),
        });
        let admission = Arc::clone(&self.admission);
        let handle = Arc::new(Handle::new(&row, now_rfc3339(), admission, None));
        lock(&handle.presented).title = begin.title;
        let fork_hold = begin.fork_hold;
        // A new session has no record yet: the backend has not been told
        // anything, so its events are the live stream's from the start. A
        // fork starts from the conversation it copies.
        let (view, events) = View::open(
            &handle.id,
            &handle.backend,
            Some(adapter.projector()),
            &begin.record,
        );
        lock(&handle.hub).install(&handle.id, view, &events);
        // Launched and registered under the lock `stop_all` takes, so a
        // session either starts before shutdown and is stopped by it, or
        // does not start at all: admission refuses the launch once Core is
        // closing, and then the session is not registered.
        let launched = {
            let mut sessions = lock(&self.sessions);
            let launched = self.launch(
                Arc::clone(&handle),
                adapter,
                transport,
                config,
                backend_version,
                Launched::New { fork_hold },
            );
            if launched.is_ok() {
                sessions.insert(handle.id.clone(), Arc::clone(&handle));
            }
            launched
        };
        match launched {
            Ok(()) => handle.session(),
            Err(refusal) => {
                self.forget_row(&id).await?;
                Err(refused_session(refusal, &workspace_id, &id))
            }
        }
    }

    /// Deletes session `id`'s row, then tells admission it is gone.
    async fn forget_row(&self, id: &str) -> Result<(), SessionError> {
        self.store.delete_session(id.to_string()).await?;
        self.admission.report(Report::SessionGone {
            session: id.to_string(),
        });
        Ok(())
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
        launched: Launched,
    ) -> Result<(), Refusal> {
        // Admission's last word on the start: refused once Core is closing,
        // and in a folder a removal holds. Admitted, the run is recorded
        // there until `run_session` lets go of it. Not waited for, so the
        // caller may hold the registry's lock.
        let run = self.admission.launch(&handle.id)?;
        let (fed_tx, fed_rx) = mpsc::unbounded_channel();
        let sink = Arc::new(CoreSink::new(
            &handle.id,
            Arc::clone(&self.app_log),
            Arc::clone(&self.admission),
            fed_tx,
        ));
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
            store: self.store.clone(),
            handle,
            live,
            adapter,
            transport,
            config,
            backend_version,
            launched,
            started: started_tx,
            finished: finished_tx,
            pipeline,
            wind_down_pause: self.wind_down_pause,
            tiles: self.tiles.clone(),
            run,
        }));
        Ok(())
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

    /// Takes session `id`'s exclusive operation from admission: the
    /// operations that change whether the session is live, which view it
    /// has, or what its row says run one at a time, and one asked for
    /// meanwhile waits here, held in admission, in the order it came. A
    /// session admission no longer has answers `not_found`. Dropping what
    /// this returns ends the operation, however the task ends.
    pub(crate) async fn exclusive(&self, id: &str) -> Result<UnderWay, SessionError> {
        let command = Command::BeginExclusive {
            session: id.to_string(),
        };
        // The one refusal there is: no such session.
        self.admission
            .command(command)
            .await
            .map_err(|_| SessionError::NotFound(id.to_string()))
    }

    /// Stops a session and waits until its backend has ended and every line
    /// is projected. Stopping a session that is not running does nothing.
    pub async fn stop(&self, id: &str) -> Result<(), SessionError> {
        let handle = self.handle(id)?;
        let _exclusive = self.exclusive(id).await?;
        stop_held(&handle).await
    }

    /// Refuses a message to a session that is being archived, as admission
    /// says (PROTO-CORE-API-R85).
    fn may_send(&self, handle: &Handle) -> Result<(), SessionError> {
        self.admission
            .may_send(&handle.id)
            .map_err(|refusal| refused_session(refusal, &handle.workspace_id, &handle.id))
    }

    /// Stops every running session, for `core.shutdown`: all at once, then
    /// waits for them together.
    pub async fn stop_all(&self) {
        let running: Vec<Arc<Live>> = {
            let sessions = lock(&self.sessions);
            // Told under the lock a launch is asked under: from here
            // admission refuses every launch, and every run it admitted
            // before is live below.
            self.admission.report(Report::Closing);
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
        // One a delete marked is finished by deleting it again.
        self.refuse_if_deleting(&handle)?;
        let workspace_id = handle.workspace_id.clone();
        let asked_of = handle.row()?;
        // Admission refuses it while a delete or a removal holds its
        // workspace, and has a delete of this session wait for it. Every
        // way out below that does not remove the session lets go, which
        // ends the removal there.
        let command = Command::RemoveSession {
            session: id.to_string(),
        };
        let removing = match self.admission.command(command).await {
            Ok(removing) => removing,
            // Another removal of it was under way when this one was asked
            // for, and took it: the session is gone, as asked.
            Err(Refusal::NoSession) => return Ok(handle.describe_from(asked_of)),
            Err(refusal) => return Err(refused_session(refusal, &workspace_id, id)),
        };
        // The removal's exclusive operation. A session whose end was
        // announced is only winding down: a client may have seen
        // `session.ended` before Core let go of the run, and can remove it,
        // so the removal waits for that, held in admission and holding
        // nothing itself, up to the stop limit. One that runs is refused:
        // stop it first.
        let _exclusive = self
            .admission
            .begin_removal(id, STOP_TIMEOUT)
            .await
            .map_err(|refusal| refused_session(refusal, &workspace_id, id))?;
        // Read before the session is gone: admission keeps no row after.
        let row = handle.row()?;
        let session = handle.describe_from(row.clone());
        self.store.delete_session(id.to_string()).await?;
        removing.end(Report::SessionGone {
            session: id.to_string(),
        });
        self.let_go(&handle, &row).await;
        Ok(session)
    }

    /// Lets go of a session whose row is gone, and was `row`: its scratch
    /// folder, what the app log, the adapter and the tiles keep of it, and
    /// its subscribers.
    async fn let_go(&self, handle: &Arc<Handle>, row: &SessionRow) {
        let id = handle.id.as_str();
        // Its scratch folder goes with it (PROTO-CORE-API-R28), once no
        // copying holds it. Never past a bound: a file system that hangs
        // leaves the folder behind, logged, rather than hold up the
        // removal.
        let cwd = row.cwd.clone();
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
        // What the adapter kept of its record goes too. A read still
        // running finds the session gone once it ends, and forgets what it
        // kept ([`read_unless_removed`]).
        if let (Some(backend_id), Some(adapter)) =
            (&row.backend_session_id, self.adapter(&row.backend))
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
            SessionError::refused(
                ErrorReason::BackendUnnamed,
                "the session's backend has not named it yet, so there is nothing to rename",
            )
        };
        // A stopped session's record is rewritten under the session's
        // exclusive operation: a start under way is waited for, and none
        // begins meanwhile, so the backend never starts on a record being
        // rewritten. A live session is renamed through its backend, which
        // needs no turn of Core's.
        let (live, exclusive) = loop {
            if let Some(live) = handle.live() {
                break (Some(live), None);
            }
            let exclusive = self.exclusive(id).await?;
            // Still stopped, or started while this waited its turn.
            if handle.live().is_none() {
                break (None, Some(exclusive));
            }
        };
        if let Some(live) = live {
            // A session still starting takes the rename once its backend is
            // ready, which for Claude Code is once the CLI has answered
            // `initialize` (PROTO-CLAUDE-CODE-STREAM-R36,
            // PROTO-CORE-API-R51): its id was chosen and stored before it
            // started, so there is a session to name already.
            let session = live.wait_ready().await?;
            // Read once ready: a backend without host-chosen ids names the
            // session as it starts.
            let backend_id = handle.row()?.backend_session_id.ok_or_else(unnamed)?;
            // The projector's count of the titles the stream announced, the
            // one tally of them, tells whether the backend's own word on the
            // new title came meanwhile.
            let announced_before = announcements(&handle);
            let requested = title.to_string();
            // A backend that is stopping refuses (not running) while it may
            // still be writing its record, so a refusal never falls back to
            // writing the record beside it: the user renames again once the
            // session has ended.
            bounded(async move { session.rename(&backend_id, &requested).await }).await?;
            // The backend announces the name it took as it takes it, and
            // that name, which it may have made unique, is the one shown
            // once the announcement is projected. Until then, the title the
            // user gave; never in place of an announcement already shown.
            // Checked and set under the hub's lock, which the pipeline holds
            // while it projects: an announcement projected before this check
            // is seen here, and one projected after it is applied after it.
            let hub = lock(&handle.hub);
            if announced(&hub) == announced_before {
                lock(&handle.presented).title = Some(title.to_string());
            }
            drop(hub);
            return self.retitled(&handle);
        } else {
            let row = handle.row()?;
            let backend_id = row.backend_session_id.clone().ok_or_else(unnamed)?;
            let adapter = self.adapter_of(&row.backend)?;
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
            let (cwd, title) = (row.cwd.clone(), title.to_string());
            let renamed = bounded(async move {
                // Held until the rewrite itself ends, however long that is
                // after this request gave up on it: the call is never
                // cancelled, and no start begins on a record still being
                // written.
                let _exclusive = exclusive;
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
                    return Err(SessionError::refused(
                        ErrorReason::RecordGone,
                        "the backend has no record of this session to rename",
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
        self.retitled(&handle)
    }

    /// Tells every client of the session's title after `session.set_title`
    /// (PROTO-CORE-API-R83), whichever client renamed it, and returns the
    /// session as it now is. One removed while its record was renamed is
    /// `not_found`, and no tile is sent for it.
    fn retitled(&self, handle: &Handle) -> Result<Session, SessionError> {
        let session = handle.session()?;
        send_tile(&self.tiles, handle, &session.tile);
        Ok(session)
    }

    /// Archives the session, or unarchives it (ADR 0026,
    /// FLOW-ARCHIVE-DELETE-SESSION). Archiving stops a live session first,
    /// as `session.stop` does, and marks it only once it has stopped: a stop
    /// that fails leaves the session as it was. Unarchiving leaves the
    /// session stopped; nothing restarts it.
    pub async fn set_archived(&self, id: &str, archived: bool) -> Result<Session, SessionError> {
        let handle = self.handle(id)?;
        self.refuse_if_deleting(&handle)?;
        // The session's exclusive operation, held from here to the mark:
        // two marks of one session land in the store and in admission in
        // the same order, and a `session.remove` (which is one too) is
        // either finished, so that there is no session to mark, or waits
        // for the mark.
        let _exclusive = if archived {
            // An archive is known to admission from when it is asked for,
            // waiting its turn or under way, and `session.send` asks
            // admission under the lock it sets `sending` with, while the
            // busy check below reads `sending` through that lock: either
            // the check sees the send, or the send sees this archive and
            // refuses (PROTO-CORE-API-R85).
            let command = Command::Archive {
                session: id.to_string(),
            };
            let archiving = self
                .admission
                .command(command)
                .await
                .map_err(|refusal| refused_session(refusal, &handle.workspace_id, id))?;
            // Refused before the stop, so a busy session is left exactly as
            // it was (PROTO-CORE-API-R77).
            if in_turn(&handle) {
                return Err(SessionError::refused(
                    ErrorReason::SessionBusy,
                    format!("session {id} is starting or working; stop it before archiving it"),
                ));
            }
            stop_held(&handle).await?;
            archiving
        } else {
            self.exclusive(id).await?
        };
        // Stored first, then kept in memory: a store that fails leaves both
        // as they were, so `session.list` never reports what a restart would
        // lose.
        self.store
            .set_session_archived(id.to_string(), archived)
            .await?;
        self.wrote(&handle, RowChange::Archived(archived))?;
        handle.session()
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
        // The session's exclusive operation, as `set_archived`: a
        // `session.remove` is either finished, so that there is no session
        // to arrange, or waits for the change. Admission also orders it
        // against a `section.remove` of the section it moves the session
        // into: one waits for the other, so the two land in the same order
        // in the store and in admission's rows (PROTO-CORE-API-R39).
        let command = Command::Arrange {
            session: id.to_string(),
            section: section_id.clone().flatten(),
        };
        let arranging = match self.admission.command(command).await {
            Err(Refusal::NoSession) => return Err(SessionError::NotFound(id.to_string())),
            answered => answered,
        };
        // A delete marks the session as its exclusive operation too, so
        // this sees a mark made while the arrangement waited; and it is
        // said before a section that does not exist is.
        self.refuse_if_deleting(&handle)?;
        let _exclusive = match arranging {
            Ok(arranging) => arranging,
            Err(Refusal::NoSection) => {
                let missing = section_id.flatten().unwrap_or_default();
                return Err(SessionError::NoSection(missing));
            }
            Err(refusal) => return Err(refused_session(refusal, &handle.workspace_id, id)),
        };
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
        let section = section_id;
        self.wrote(&handle, RowChange::Arranged { pinned, section })?;
        handle.session()
    }

    /// Removes section `id`, putting its sessions in no section
    /// (PROTO-CORE-API-R39). Returns the section as it was; `None` when
    /// there is no such section.
    pub async fn remove_section(&self, id: &str) -> Result<Option<Section>, SessionError> {
        // Admission knows which sections there are, and holds this while
        // an arrangement into the section is under way. A store that fails
        // drops the removal, which leaves the section as it was.
        let section = id.to_string();
        let command = Command::RemoveSection {
            section: section.clone(),
        };
        let Ok(removing) = self.admission.command(command).await else {
            return Ok(None);
        };
        let removed = self.store.delete_section(section.clone()).await?;
        // One step: no row is left naming the section.
        removing.end(Report::SectionRemoved { section });
        Ok(removed)
    }

    // -- Deleting (PROTO-CORE-API, `session.delete`) --------------------------

    /// The session's row as last stored.
    pub fn row(&self, id: &str) -> Result<SessionRow, SessionError> {
        self.handle(id)?.row()
    }

    /// Tells admission a change to the session's row is stored. A session
    /// that is gone keeps nothing, and its request answers `not_found`.
    fn wrote(&self, handle: &Handle, change: RowChange) -> Result<(), SessionError> {
        if self.admission.wrote(&handle.id, change) {
            Ok(())
        } else {
            Err(SessionError::NotFound(handle.id.clone()))
        }
    }

    /// Refuses a delete while the session is starting, a turn is running
    /// in it (one waiting on the user included), or a message is being
    /// sent to it (FLOW-ARCHIVE-DELETE-SESSION-R05). `session.delete` asks
    /// once it holds the claim, so a send cannot start in between.
    pub fn check_deletable(&self, id: &str) -> Result<(), SessionError> {
        let handle = self.handle(id)?;
        // The fork's CLI has yet to read the record a delete would take.
        if handle.forking.load(Ordering::SeqCst) {
            return Err(SessionError::InvalidState(format!(
                "session {id} is being forked; delete it once the fork has started"
            )));
        }
        if in_turn(&handle) {
            return Err(SessionError::refused(
                ErrorReason::SessionBusy,
                format!("session {id} is starting or working; stop it before deleting it"),
            ));
        }
        Ok(())
    }

    /// Holds session `id`'s workspace for its `session.delete`: admitted
    /// by admission, which refuses it `busy` while another delete or a
    /// removal holds the workspace. Dropping what this returns ends the
    /// delete there and releases the workspace.
    pub async fn hold_for_delete(&self, id: &str) -> Result<UnderWay, SessionError> {
        let workspace_id = self.row(id)?.workspace_id;
        let command = Command::DeleteSession {
            session: id.to_string(),
        };
        self.admission
            .command(command)
            .await
            .map_err(|refusal| refused_session(refusal, &workspace_id, id))
    }

    /// Refuses a request that would change workspace `workspace_id` while a
    /// delete or a removal holds it, as admission says.
    fn refuse_if_claimed(&self, workspace_id: &str) -> Result<(), SessionError> {
        if self.admission.workspace_held(workspace_id) {
            return Err(busy(workspace_id));
        }
        Ok(())
    }

    /// Refuses a request about session `handle` while a delete is under
    /// way for it, as admission says, or its workspace went with one or with
    /// a removal; or it is marked deleting.
    fn refuse_if_deleting(&self, handle: &Handle) -> Result<(), SessionError> {
        // One that is gone is not marked: its workspace says the rest.
        let deleting = handle.stored_row().and_then(|row| row.deleting);
        let workspace_id = &handle.workspace_id;
        if self.admission.deleting(workspace_id, &handle.id) {
            return Err(busy(workspace_id));
        }
        if deleting.is_some() {
            return Err(SessionError::refused(
                ErrorReason::SessionDeleting,
                format!(
                    "session {} is being deleted; delete it again to finish",
                    handle.id
                ),
            ));
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
        let adapter = self.adapter_of(&row.backend)?;
        Ok(ask_open_elsewhere(adapter, transport, &backend_id, ()).await)
    }

    /// Marks the session deleting, `running`, and records whether its
    /// delete takes the workspace's files: `files`, unless an earlier try
    /// recorded that they stay. Returns what is recorded. Stored first, then
    /// kept in memory, as an archive mark is.
    pub async fn mark_deleting(&self, id: &str, files: bool) -> Result<bool, SessionError> {
        let handle = self.handle(id)?;
        let _exclusive = self.exclusive(id).await?;
        let recorded = self
            .store
            .mark_session_deleting(id.to_string(), files)
            .await?;
        self.wrote(&handle, RowChange::Deleting(Deleting::Running))?;
        let workspace = handle.workspace_id.clone();
        // The store set the workspace to `error` with the mark.
        if recorded {
            self.admission
                .report(Report::WorkspaceErrored { workspace });
        }
        Ok(recorded)
    }

    /// Marks the session's delete failed, for the user to try again.
    pub async fn mark_delete_failed(&self, id: &str) -> Result<(), SessionError> {
        let handle = self.handle(id)?;
        let _exclusive = self.exclusive(id).await?;
        self.store.set_session_delete_failed(id.to_string()).await?;
        self.wrote(&handle, RowChange::Deleting(Deleting::Failed))
    }

    /// Deletes the session's record from its backend, on the host
    /// `transport` reaches (PROTO-CORE-API-R36), giving up after `limit`. A
    /// session its backend never named has no record to delete.
    ///
    /// However this returns, the deletion is called off on the way out
    /// (PROTO-CORE-API-R91): when Core gives up on it, or this request is
    /// dropped, the adapter begins no further deletion, so a record the user
    /// was told is kept is not deleted later. Calling off one that finished
    /// changes nothing.
    pub async fn delete_record(
        &self,
        id: &str,
        transport: Arc<dyn Transport>,
        limit: Duration,
    ) -> Result<RecordDeletion, SessionError> {
        let row = self.row(id)?;
        let Some(backend_id) = row.backend_session_id else {
            return Ok(RecordDeletion::Deleted);
        };
        let adapter = self.adapter_of(&row.backend)?;
        let cwd = row.cwd;
        let call_off = CallOff::default();
        let _called_off_on_return = CallOffOnDrop(call_off.clone());
        within(limit, "the session's record was not deleted", async move {
            adapter
                .delete_record(transport, &cwd, &backend_id, &call_off)
                .await
        })
        .await
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
        let _listing = self.listing.lock().await;
        let _exclusive = self.exclusive(id).await?;
        // Read before the session is gone: admission keeps no row after.
        let row = handle.row()?;
        let workspace_went = self
            .store
            .finish_session_delete(id.to_string(), workspace_id.clone())
            .await?;
        self.admission.report(Report::SessionGone {
            session: id.to_string(),
        });
        if let (true, Some(workspace)) = (workspace_went, workspace_id) {
            self.admission
                .report(Report::WorkspaceDeleted { workspace });
        }
        self.deletes.fetch_add(1, Ordering::SeqCst);
        self.let_go(&handle, &row).await;
        Ok(())
    }

    /// Takes effect on the session's next start.
    pub async fn set_mode(&self, id: &str, mode: String) -> Result<Session, SessionError> {
        let handle = self.handle(id)?;
        // The session's exclusive operation, as any stored change takes:
        // one that is gone is `not_found`, and nothing is written for it.
        let _exclusive = self.exclusive(id).await?;
        self.store
            .set_session_mode(id.to_string(), mode.clone())
            .await?;
        self.wrote(&handle, RowChange::Mode(mode))?;
        handle.session()
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

    /// Answers the question `request_id` the session's backend is waiting on,
    /// with the user's answers or their declining it (PROTO-CORE-API-R33).
    /// The `question.answered` event follows from the line the adapter
    /// writes, as `permission.decided` does from a decision.
    pub async fn answer_question(
        &self,
        id: &str,
        request_id: &str,
        reply: QuestionReply,
    ) -> Result<(), SessionError> {
        let live = self.live(id)?;
        let session = live.wait_ready().await?;
        let request_id = request_id.to_string();
        bounded(async move { session.answer_question(&request_id, reply).await }).await
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
            // Opened outside the hub's lock, one opening at a time: it is
            // the session's exclusive operation.
            let _exclusive = self.exclusive(&handle.id).await?;
            if handle.live().is_some() || !wants_view(&lock(&handle.hub), false) {
                continue;
            }
            opened = Some(self.open_view(&handle, &transport_for).await?);
        };
        if installed {
            send_current_tile(&self.tiles, &handle);
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
        let row = handle.row()?;
        let adapter = self.adapter_of(&row.backend)?;
        let source = match transport_for(&row.host_id) {
            Some(transport) => {
                record_id(&self.store, handle, Arc::clone(&adapter), transport, &row).await
            }
            None => row.backend_session_id.clone(),
        };
        let record = match &source {
            // The backend never announced the session, so it has no record.
            None => Arc::default(),
            Some(backend_id) => {
                let read = match transport_for(&row.host_id) {
                    Some(transport) => {
                        let (reader, cwd, id) =
                            (Arc::clone(&adapter), row.cwd.clone(), backend_id.clone());
                        let at = Arc::clone(&transport);
                        let gone = handle.when_gone();
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
                                read_unless_removed(read, gone, || reader.forget_record(&cwd, &id))
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
        // Read from its original, a fork's conversation stops where it did,
        // and is gone with that record.
        let record = match &row.forked_through {
            Some(last) if source != row.backend_session_id && record.is_empty() => {
                Arc::new(vec![RecordEvent {
                    message_id: Some(last.clone()),
                    ts: Some(now_rfc3339()),
                    backend_version: None,
                    event: Event::Error(ErrorEvent {
                        code: ErrorCode::Transport,
                        message: "this fork's conversation was in the session it was forked \
                                  from, and that session's record is gone"
                            .into(),
                        recoverable: false,
                        action: None,
                    }),
                }])
            }
            Some(last) if source != row.backend_session_id => up_to(record, last),
            _ => record,
        };
        // Turns taken elsewhere may have changed its title too.
        if let (Some(backend_id), Some(transport)) = (source, transport_for(&row.host_id)) {
            apply_summary(
                handle,
                &self.tiles,
                adapter,
                transport,
                row.cwd.clone(),
                backend_id,
            )
            .await;
        }
        let (mut view, mut events) = View::open(&row.id, &row.backend, None, &record);
        if let Some((fact, ts)) = lock(&handle.ending).clone() {
            events.extend(view.lifecycle(&fact, &ts));
        }
        Ok((view, events))
    }
}

/// How many titles the session's live stream has announced so far, as its
/// projector counts them; `None` before any.
fn announcements(handle: &Handle) -> Option<u64> {
    announced(&lock(&handle.hub))
}

fn announced(hub: &Hub) -> Option<u64> {
    hub.view
        .as_ref()
        .and_then(|view| view.announced_title())
        .map(|(count, _)| count)
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
    tiles: &TileHub,
    adapter: Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
    cwd: String,
    backend_id: String,
) {
    let gone = handle.when_gone();
    let live = handle.live().is_some();
    // Nothing is read for a session that is gone.
    let Some(row) = handle.stored_row() else {
        return;
    };
    // A fork with no record of its own was given its original's to read for
    // the conversation. What that record says of its title and first time is
    // the original's, and changes with it.
    if row.forked_from.as_deref() == Some(backend_id.as_str()) {
        return;
    }
    let start = move || {
        tokio::spawn(async move {
            let read = adapter.summarize_record(transport, &cwd, &backend_id);
            read_unless_removed(read, gone, || adapter.forget_record(&cwd, &backend_id))
                .await
                .map_err(|e| e.to_string())
        })
    };
    // A summary is small and the adapter keeps them by the record's state,
    // so a joiner simply reads again.
    match read_once(&handle.summary_read, SUMMARY_READ, start, async |_| false).await {
        Some(Ok(Some(summary))) => {
            let changed = lock(&handle.presented).take_summary(summary, live, &handle.id);
            if changed {
                send_current_tile(tiles, handle);
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

/// Reads session `row`'s title and creation time from the record that
/// holds its conversation ([`record_id`]).
async fn read_summary(
    store: &Store,
    handle: &Handle,
    tiles: &TileHub,
    adapter: Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
    row: SessionRow,
) {
    let read = Arc::clone(&transport);
    if let Some(id) = record_id(store, handle, Arc::clone(&adapter), read, &row).await {
        apply_summary(handle, tiles, adapter, transport, row.cwd, id).await;
    }
}

/// Whose record holds session `row`'s conversation: its own, or for a fork
/// the backend has not written a record for yet (it does with the first
/// message), its original's. A record that can't be looked at counts as its
/// own, which then says what is wrong with it. A fork found to have a record
/// has no more use for its original's, so the link goes (FLOW-FORK-SESSION-R09).
async fn record_id(
    store: &Store,
    handle: &Handle,
    adapter: Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
    row: &SessionRow,
) -> Option<String> {
    let own = row.backend_session_id.clone()?;
    let Some(original) = &row.forked_from else {
        return Some(own);
    };
    let version = adapter.record_version(transport, &row.cwd, &own);
    match tokio::time::timeout(RECORD_READ, version).await {
        Ok(Ok(None)) => Some(original.clone()),
        Ok(Ok(Some(_))) => {
            match store.clear_fork_link(handle.id.clone()).await {
                Ok(()) => {
                    let change = RowChange::ForkLinkCleared;
                    handle.admission.wrote(&handle.id, change);
                }
                Err(e) => {
                    tracing::warn!(session = %handle.id, error = %e, "could not forget the session a fork was copied from");
                }
            }
            Some(own)
        }
        _ => Some(own),
    }
}

/// The events of `record` up to and including those of message `last`, all
/// of them when it holds no such message.
fn up_to(record: Arc<Vec<RecordEvent>>, last: &str) -> Arc<Vec<RecordEvent>> {
    match record
        .iter()
        .rposition(|event| event.message_id.as_deref() == Some(last))
    {
        Some(at) => Arc::new(record[..=at].to_vec()),
        None => record,
    }
}

/// The processes a registry named, as a message says them: `pid 7`, or
/// `pids 7, 9`.
pub(crate) fn pids_named(pids: &[u32]) -> String {
    match pids.len() {
        1 => format!("pid {}", pid_list(pids)),
        _ => format!("pids {}", pid_list(pids)),
    }
}

/// The pids alone, as `7, 9`: `open_elsewhere`'s `detail`, and what
/// [`pids_named`] says.
fn pid_list(pids: &[u32]) -> String {
    let list: Vec<String> = pids.iter().map(u32::to_string).collect();
    list.join(", ")
}

/// Asks `adapter` whether another process has session `backend_id` open,
/// for at most [`OPEN_ELSEWHERE_CHECK`]: a check that panics or hangs could
/// not tell, so its answer is unknown. `held` is kept until the check itself
/// ends, however long that is after the answer.
async fn ask_open_elsewhere(
    adapter: Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
    backend_id: &str,
    held: impl Send + 'static,
) -> OpenElsewhere {
    let id = backend_id.to_string();
    let check = tokio::spawn(async move {
        let _held = held;
        adapter.open_elsewhere(transport, &id).await
    });
    match tokio::time::timeout(OPEN_ELSEWHERE_CHECK, check).await {
        Ok(Ok(answer)) => answer,
        Ok(Err(_)) | Err(_) => OpenElsewhere::Unknown,
    }
}

/// Asks the adapter whether a process other than this Core's has the
/// session open, before Core resumes it or writes to its record, and when
/// one has, or the adapter cannot tell, adds a `session.notice` saying so to
/// the session's open view (PROTO-AGENT-EVENTS-R14). It never stops what
/// follows. `consequence` says what the other process does meanwhile.
///
/// Notices are not part of the record, so one reaches the clients reading
/// the session now; with none reading, there is nobody to tell.
async fn notice_if_open_elsewhere(
    handle: &Handle,
    adapter: Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
    backend_id: &str,
    consequence: &str,
) {
    let notice = match ask_open_elsewhere(adapter, transport, backend_id, ()).await {
        OpenElsewhere::No => return,
        OpenElsewhere::Yes { pids } => SessionNotice {
            level: NoticeLevel::Warning,
            code: NoticeCode::SessionOpenElsewhere,
            message: format!(
                "Another process ({}) has this session open, and {consequence}.",
                pids_named(&pids)
            ),
            pid: pids.first().copied(),
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
/// The call is never cancelled: dropping a write halfway would leave half a
/// line in the backend's stdin, which the next line would be glued onto. A
/// call still stuck when the request gives up finishes, or fails, when the
/// session is stopped.
async fn bounded(
    call: impl std::future::Future<Output = Result<(), AdapterError>> + Send + 'static,
) -> Result<(), SessionError> {
    within(ADAPTER_CALL, "the backend did not take the line", call).await
}

/// Runs one adapter call in a task of its own, answering after at most
/// `limit`; `timed_out` says what had not happened by then. The task goes on
/// after the answer: a call that must stop then is told so by its caller
/// (as [`SessionManager::delete_record`] calls its deletion off).
async fn within<T: Send + 'static>(
    limit: Duration,
    timed_out: &str,
    call: impl std::future::Future<Output = Result<T, AdapterError>> + Send + 'static,
) -> Result<T, SessionError> {
    match tokio::time::timeout(limit, tokio::spawn(call)).await {
        Ok(Ok(result)) => result.map_err(SessionError::Adapter),
        Ok(Err(e)) => Err(SessionError::Internal(format!("the adapter failed: {e}"))),
        Err(_) => Err(SessionError::Adapter(AdapterError::Io(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("{timed_out} within {limit:?}"),
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
    /// Told that the run is ending, before the fact that says so is queued.
    admission: Arc<Admission>,
    inner: Mutex<SinkInner>,
    /// Set by `session.stop`: the exit that follows is the user's doing.
    stop_requested: AtomicBool,
    /// Becomes true when the adapter reports the process exit, even one held
    /// back because the start had not finished.
    exit_seen: watch::Sender<bool>,
    /// Becomes true when the fact that ends the session is queued.
    ended: watch::Sender<bool>,
}

tokio::task_local! {
    /// How many host lines the adapter recorded within one call that counts
    /// them ([`counting_host_lines`]). An adapter records each line before
    /// writing it, from within the call that writes it, so the count tells
    /// that call's refusal before writing from a write that may have reached
    /// the backend, whatever other requests write meanwhile.
    static HOST_LINES_WRITTEN: Arc<AtomicU64>;
}

/// Runs `call`, counting the host lines recorded within it in `written`.
fn counting_host_lines<F: std::future::Future>(
    written: Arc<AtomicU64>,
    call: F,
) -> tokio::task::futures::TaskLocalFuture<Arc<AtomicU64>, F> {
    HOST_LINES_WRITTEN.scope(written, call)
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
    fn new(
        session_id: &str,
        app_log: Arc<AppLog>,
        admission: Arc<Admission>,
        fed: mpsc::UnboundedSender<Fed>,
    ) -> Self {
        CoreSink {
            session_id: session_id.to_string(),
            app_log,
            admission,
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
        if dir == Direction::Host {
            // Counted for the adapter call recording it, when one counts.
            let _ = HOST_LINES_WRITTEN.try_with(|written| written.fetch_add(1, Ordering::SeqCst));
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

    /// The session's task is letting go of the run: nothing ends it from
    /// here. Admission is told the run is ending if nothing said so yet (a
    /// pipeline that died with no exit), and an exit reported after this is
    /// ignored, so it is never taken for the end of a later run.
    fn let_go(&self) {
        let mut inner = lock(&self.inner);
        if !inner.ended {
            inner.ended = true;
            self.admission.report(Report::RunEnding {
                session: self.session_id.clone(),
            });
        }
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
        self.end_with(&mut inner, fact);
        drop(inner);
        self.ended.send_replace(true);
    }

    /// Queues `fact`, which ends the session. Admission is told the run is
    /// ending before the fact is queued: the pipeline may publish it at
    /// once, and a client that has seen `session.ended` must find the run
    /// ending, not running.
    fn end_with(&self, inner: &mut SinkInner, fact: LifecycleFact) {
        self.admission.report(Report::RunEnding {
            session: self.session_id.clone(),
        });
        self.push_fact(inner, fact);
        inner.ended = true;
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
            self.end_with(&mut inner, fact);
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

/// Why Core starts a session's backend.
enum Launched {
    /// A new session, or a fork with its hold on the original, let go once
    /// the start is over.
    New { fork_hold: Option<Arc<ForkHold>> },
    /// A stopped session started again (FLOW-RESUME-SESSION), and when it
    /// is a fork made again, its hold on the original, let go likewise.
    Resumed { fork_hold: Option<Arc<ForkHold>> },
}

struct SessionTask {
    store: Store,
    handle: Arc<Handle>,
    live: Arc<Live>,
    adapter: Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
    config: SessionConfig,
    backend_version: String,
    launched: Launched,
    started: watch::Sender<Started>,
    finished: watch::Sender<bool>,
    pipeline: tokio::task::JoinHandle<()>,
    wind_down_pause: Duration,
    /// Where `session.tile` goes when the record read at the end retitles
    /// the session (PROTO-CORE-API-R103).
    tiles: Arc<TileHub>,
    /// The run as admission has it, from the launch until this task lets go
    /// of it, however the task ends.
    run: UnderWay,
}

async fn run_session(task: SessionTask) {
    let SessionTask {
        store,
        handle,
        live,
        adapter,
        transport,
        config,
        backend_version,
        launched,
        started,
        finished,
        pipeline,
        wind_down_pause,
        tiles,
        run,
    } = task;
    // Ready with no turn to report one: a fork, and a session started again,
    // whose backend reports nothing until its next message.
    let (fork_hold, ready_fact) = match launched {
        Launched::New { fork_hold } => {
            let fact = fork_hold.is_some().then_some(LifecycleFact::ForkReady);
            (fork_hold, fact)
        }
        Launched::Resumed { fork_hold } => (fork_hold, Some(LifecycleFact::Resumed)),
    };
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
    drop(fork_hold);
    let mut pipeline = pipeline;
    let mut pipeline_done = false;
    match outcome {
        Some(Ok(session)) => {
            let session: Arc<dyn AdapterSession> = Arc::from(session);
            *lock(&handle.start_failure) = None;
            // Before the session can be sent anything, so a first message's
            // lines come after this.
            if let Some(fact) = ready_fact {
                sink.record_fact(fact);
            }
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
    if !wind_down_pause.is_zero() {
        tokio::time::sleep(wind_down_pause).await;
    }
    // Ending, if nothing said so yet: a start or a removal asked for from
    // here waits for the run's end below rather than finding the session
    // running.
    sink.let_go();
    *lock(&handle.live) = None;
    // Core has let go of the run. Admission is told after `live` is
    // cleared, so what waited for the run's end finds none.
    drop(run);
    // Nobody reading it: nothing more will be added, so it goes.
    handle.close_if_idle();
    finished.send_replace(true);
    // The record now holds the session's turns, and with them its title.
    // Read after the stop has its answer, never on its way.
    // A removal that waited for the run's end may have gone ahead since:
    // then there is no record of Ersa's to read.
    if let Some(row) = handle.stored_row() {
        tokio::spawn(async move {
            read_summary(&store, &handle, &tiles, adapter, transport, row).await;
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
    let mut last_tile = handle.state_and_tile().1;
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
        let (events, tile, turn, announced, retitled, reported) = {
            let mut hub = lock(&handle.hub);
            let Some(view) = hub.view.as_mut() else {
                // Never: a live session's view is open from its start.
                tracing::error!(session = %handle.id, "a live session has no view; its events are lost");
                continue;
            };
            let title_before = view.announced_title().map(|(count, _)| count);
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
            let reported = settings::Reported {
                model: view.reported_model().map(String::from),
                permission_mode: view.reported_permission_mode().map(String::from),
            };
            // Any announcement in this batch, the same title again included:
            // the latest is the session's.
            let retitled = view
                .announced_title()
                .filter(|(count, _)| title_before != Some(*count))
                .map(|(_, title)| title.to_string());
            let (tile, turn) = (view.tile().clone(), view.turn());
            (events, tile, turn, announced, retitled, reported)
        };

        // What the stream reports of the model and permission mode is the
        // session's from now on, over Core's request and the record
        // (PROTO-CORE-API-R71).
        {
            let mut presented = lock(&handle.presented);
            if reported.model.is_some() {
                presented.reported.model = reported.model;
            }
            if reported.permission_mode.is_some() {
                presented.reported.permission_mode = reported.permission_mode;
            }
        }

        // A title the backend announced is the session's from now on, until
        // a newer one or its record says otherwise (PROTO-CLAUDE-CODE-STREAM-R36).
        let title_changed = retitled.is_some();
        if let Some(title) = retitled {
            lock(&handle.presented).title = Some(title);
        }

        // The backend's own id finds the session's record later; it is the
        // one thing about the session's history Ersa keeps. Stored before
        // anyone hears of the session's events, so a crash after a client
        // saw them cannot leave a session whose record is lost.
        if let Some(backend_id) = announced {
            let known = handle.stored_row().and_then(|row| row.backend_session_id);
            if known.as_deref() != Some(backend_id.as_str()) {
                match store
                    .set_backend_session_id(handle.id.clone(), backend_id.clone())
                    .await
                {
                    Ok(()) => {
                        let change = RowChange::BackendId(backend_id);
                        handle.admission.wrote(&handle.id, change);
                    }
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
                    let change = RowChange::LastActivity(at);
                    handle.admission.wrote(&handle.id, change);
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
        // A new title, the backend's announced or generated name, sends one
        // too (PROTO-CORE-API-R83).
        if tile != last_tile || activity_changed || title_changed {
            send_tile(&tiles, &handle, &tile);
            last_tile = tile;
        }
        progress.send_replace(Progress {
            lines: lines_done,
            turn,
            holding: last_tile.waiting.is_some(),
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

/// Sends the session's tile as it now is.
fn send_current_tile(tiles: &TileHub, handle: &Handle) {
    send_tile(tiles, handle, &handle.state_and_tile().1);
}

/// Sends a session's tile with its last activity, title and date created,
/// which every tile carries as `session.list` has them. None is sent for a
/// session that is gone.
fn send_tile(tiles: &TileHub, handle: &Handle, tile: &Tile) {
    let Some(row) = handle.stored_row() else {
        return;
    };
    let last_activity_at = row.last_activity_at;
    let presented = lock(&handle.presented).clone();
    // Every tile carries the title and the date created, so a coalesced
    // tile never loses a change to either (PROTO-CORE-API-R83, R105).
    let params = SessionTileParams {
        session_id: handle.id.clone(),
        tile: tile.clone(),
        last_activity_at,
        title: presented.title,
        created_at: presented.created_at,
    };
    tiles.publish(
        &handle.id,
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

    /// A record read that corrects the date created asks for a tile; the
    /// same read again, with nothing changed, asks for none.
    // verifies: PROTO-CORE-API-R106
    #[test]
    fn a_corrected_date_created_asks_for_one_tile_and_a_reread_for_none() {
        let mut presented = Presented {
            title: Some("Same".into()),
            created_at: "2026-11-15T09:00:00.000Z".into(),
            recorded: Default::default(),
            reported: Default::default(),
        };
        let summary = || RecordSummary {
            title: Some("Same".into()),
            created_at: Some("2026-11-01T00:00:00.000Z".into()),
            settings: Default::default(),
        };
        assert!(presented.take_summary(summary(), false, "s-1"));
        assert_eq!(presented.created_at, "2026-11-01T00:00:00.000Z");
        assert!(!presented.take_summary(summary(), false, "s-1"));
    }

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
        /// A rename of a record waits here until the test lets it go.
        rename: Notify,
        forgets: std::sync::atomic::AtomicUsize,
        stats_hang: AtomicBool,
        hung_stats: std::sync::atomic::AtomicUsize,
    }

    #[ersa_adapter::async_trait]
    impl Adapter for GatedReads {
        fn backend(&self) -> &str {
            "gated"
        }
        fn title_rules(&self) -> ersa_adapter::TitleRules {
            ersa_adapter::TitleRules {
                max_chars: 10,
                one_line: true,
            }
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
        async fn rename_record(
            &self,
            _transport: Arc<dyn Transport>,
            _cwd: &str,
            _backend_session_id: &str,
            _title: &str,
        ) -> Result<(), AdapterError> {
            self.rename.notified().await;
            Ok(())
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
            forked_from: None,
            forked_through: None,
            model_at: None,
            effort_at: None,
            permission_mode_at: None,
            started_elsewhere: false,
        };
        // Admission has the session and its workspace, as once it is
        // created: a removal is decided there.
        let admission = Arc::new(Admission::default());
        admission.restore(crate::admission::Existing {
            projects: vec![(
                "p".into(),
                crate::admission::Folder {
                    host: "local".into(),
                    key: "c:/work".into(),
                },
            )],
            workspaces: vec![crate::admission::ExistingWorkspace {
                id: "w".into(),
                project: "p".into(),
                root: true,
                phase: crate::admission::WorkspacePhase::Ready,
            }],
            sessions: vec![row.clone()],
            sections: Vec::new(),
        });
        let manager = SessionManager::new(
            store,
            vec![Arc::clone(adapter) as Arc<dyn Adapter>],
            Arc::new(app_log),
            Arc::clone(&admission),
        );
        let handle = Arc::new(Handle::new(&row, now_rfc3339(), admission, None));
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

    /// Makes `s-gated` live by hand, as a launch does, and returns its
    /// sink, the sender that marks it finished, and the run as admission
    /// has it, which the session task drops when it lets go of the run.
    fn make_live(manager: &SessionManager) -> (Arc<CoreSink>, watch::Sender<bool>, UnderWay) {
        let handle = manager.handle("s-gated").unwrap();
        let run = manager.admission.launch("s-gated").unwrap();
        let sink = Arc::new(CoreSink::new(
            "s-gated",
            Arc::clone(&manager.app_log),
            Arc::clone(&manager.admission),
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
        (sink, finished, run)
    }

    #[tokio::test]
    async fn an_exit_reported_after_its_run_was_let_go_of_does_not_end_a_later_run() {
        let adapter = Arc::new(GatedReads::default());
        let dir = tempfile::tempdir().unwrap();
        let manager = gated_manager(dir.path(), &adapter);
        // A run whose backend never reported its exit: its pipeline died,
        // the stop was not answered in time, and the task let go of it.
        let (old_sink, _finished, old_run) = make_live(&manager);
        old_sink.start_finished(Ok(()));
        old_sink.let_go();
        drop(old_run);
        // The session is started again.
        let (_sink, _finished, _run) = make_live(&manager);
        let run = |manager: &SessionManager| {
            manager
                .admission
                .snapshot()
                .session("s-gated")
                .map(|session| session.run)
        };
        assert_eq!(run(&manager), Some(crate::admission::Run::Running));
        // The old backend exits at last: that is not the new run's end.
        old_sink.process_exited(Ok(ProcessExit { code: Some(0) }));
        assert_eq!(run(&manager), Some(crate::admission::Run::Running));
    }

    // verifies: ARCH-CORE-R14
    #[tokio::test]
    async fn a_session_s_row_is_the_one_admission_keeps() {
        let adapter = Arc::new(GatedReads::default());
        let dir = tempfile::tempdir().unwrap();
        let manager = gated_manager(dir.path(), &adapter);
        let handle = manager.handle("s-gated").unwrap();
        assert_eq!(
            manager.row("s-gated").ok(),
            manager.admission.row("s-gated")
        );

        // A stored change is told to admission, and read from it.
        let session = manager.set_mode("s-gated", "plan".into()).await.unwrap();
        assert_eq!(session.mode.as_deref(), Some("plan"));
        let kept = manager.admission.row("s-gated").unwrap();
        assert_eq!(kept.mode.as_deref(), Some("plan"));
        assert_eq!(
            manager.get("s-gated").unwrap().mode.as_deref(),
            Some("plan")
        );

        // The session goes: admission keeps no row, and Core has let go of
        // nothing yet.
        manager.admission.report(Report::SessionGone {
            session: "s-gated".into(),
        });
        // A write for it keeps nothing, and its request is `not_found`.
        let written = manager.set_mode("s-gated", "build".into()).await;
        assert!(
            matches!(written, Err(SessionError::NotFound(_))),
            "{written:?}"
        );
        assert_eq!(manager.admission.row("s-gated"), None);
        // It is not listed, and a request that asks for it finds none.
        assert!(manager.list().is_empty());
        assert!(matches!(
            manager.get("s-gated"),
            Err(SessionError::NotFound(_))
        ));
        assert!(matches!(
            manager.row("s-gated"),
            Err(SessionError::NotFound(_))
        ));
        assert!(handle.describe().is_none());
    }

    /// Stores `s-gated`'s row, with its project and workspace, so a write
    /// to it finds a row.
    async fn store_gated(manager: &SessionManager) {
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
        let key = "c:/work".into();
        store
            .insert_project(project, "w".into(), key)
            .await
            .unwrap();
        let row = manager.row("s-gated").unwrap();
        store.insert_session(row).await.unwrap();
    }

    // verifies: ARCH-CORE-R14
    #[tokio::test]
    async fn a_delete_s_marks_are_told_to_admission_and_one_for_a_session_that_is_gone_is_not_found()
     {
        let adapter = Arc::new(GatedReads::default());
        let dir = tempfile::tempdir().unwrap();
        let manager = gated_manager(dir.path(), &adapter);
        store_gated(&manager).await;
        let deleting = |manager: &SessionManager| manager.get("s-gated").unwrap().deleting;
        assert_eq!(deleting(&manager), None);
        manager.mark_deleting("s-gated", true).await.unwrap();
        assert_eq!(deleting(&manager), Some(Deleting::Running));
        manager.mark_delete_failed("s-gated").await.unwrap();
        assert_eq!(deleting(&manager), Some(Deleting::Failed));
        let kept = manager.admission.row("s-gated").unwrap();
        let stored = manager.store.session("s-gated".into()).await.unwrap();
        assert_eq!(Some(kept), stored);

        // Gone: the mark is kept nowhere, and its request is `not_found`.
        manager.admission.report(Report::SessionGone {
            session: "s-gated".into(),
        });
        let marked = manager.mark_delete_failed("s-gated").await;
        assert!(
            matches!(marked, Err(SessionError::NotFound(_))),
            "{marked:?}"
        );
    }

    // verifies: FLOW-RESUME-SESSION-R13
    #[tokio::test]
    async fn found_sessions_whose_rows_went_with_their_workspace_are_let_go_of() {
        let adapter = Arc::new(GatedReads::default());
        let dir = tempfile::tempdir().unwrap();
        let manager = gated_manager(dir.path(), &adapter);
        // A session found in the same folder, stored and loaded.
        let mut found = manager.row("s-gated").unwrap();
        found.id = "s-found".into();
        found.backend_session_id = Some("b-found".into());
        found.started_elsewhere = true;
        let command = Command::CreateSession {
            workspace: "w".into(),
            session: "s-found".into(),
            found: true,
        };
        let admitted = manager.admission.command(command).await.unwrap();
        admitted.end(Report::SessionStored {
            row: Box::new(found.clone()),
        });
        let admission = Arc::clone(&manager.admission);
        let handle = Arc::new(Handle::new(&found, now_rfc3339(), admission, None));
        lock(&manager.sessions).insert(handle.id.clone(), handle);

        // As a removal does: quiet them, take their rows, then let go.
        let quiet = manager.quiet_found("w").await;
        manager.admission.report(Report::SessionGone {
            session: "s-found".into(),
        });
        manager.forget_found(quiet).await;
        assert!(matches!(
            manager.handle("s-found"),
            Err(SessionError::NotFound(_))
        ));
        // What the adapter kept of its record went too, by the row as it was.
        assert_eq!(adapter.forgets.load(Ordering::SeqCst), 1);
        // One Ersa started is not theirs to take.
        assert!(manager.handle("s-gated").is_ok());
    }

    // verifies: ARCH-CORE-R14
    #[tokio::test]
    async fn a_write_to_a_stopped_session_waits_for_its_exclusive_operation() {
        let adapter = Arc::new(GatedReads::default());
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(gated_manager(dir.path(), &adapter));
        store_gated(&manager).await;
        // The rename's rewrite, once it has its turn, is not held up here.
        adapter.rename.notify_one();
        type Write = fn(Arc<SessionManager>) -> tokio::task::JoinHandle<bool>;
        let writes: [(&str, Write); 3] = [
            ("a mode change", |manager| {
                tokio::spawn(async move {
                    let mode = "plan".to_string();
                    manager.set_mode("s-gated", mode).await.is_ok()
                })
            }),
            ("a delete's failed mark", |manager| {
                tokio::spawn(async move { manager.mark_delete_failed("s-gated").await.is_ok() })
            }),
            // Its record is rewritten, so no start may begin meanwhile.
            ("a rename", |manager| {
                tokio::spawn(async move {
                    let transport: Arc<dyn Transport> = Arc::new(ersa_adapter::LocalTransport);
                    let found = |_: &str| Some(transport);
                    // Whatever the adapter answers: it was asked.
                    let _ = manager.set_title("s-gated", "Renamed", found).await;
                    true
                })
            }),
        ];
        for (name, write) in writes {
            // Something else has the session: a start, say.
            let held = manager.exclusive("s-gated").await.unwrap();
            let writing = write(Arc::clone(&manager));
            // The write waits its turn, held in admission.
            let wanted = Command::BeginExclusive {
                session: "s-gated".into(),
            };
            assert_eq!(held_in(&manager).await, [wanted], "{name}");
            assert!(!writing.is_finished(), "{name}");
            // With its turn it goes on, and lets go when it is done.
            drop(held);
            let done = tokio::time::timeout(Duration::from_secs(5), writing).await;
            assert!(done.expect("ended in time").expect("joined"), "{name}");
            let state = manager.admission.snapshot();
            assert_eq!(state.session("s-gated").unwrap().exclusive, None, "{name}");
        }
        assert_eq!(
            manager.get("s-gated").unwrap().mode.as_deref(),
            Some("plan")
        );
    }

    // verifies: ARCH-CORE-R16
    // Paused time: the request's limit passes when the test's clock says so.
    #[tokio::test(start_paused = true)]
    async fn a_rename_given_up_on_holds_its_session_until_the_record_is_written() {
        let adapter = Arc::new(GatedReads::default());
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(gated_manager(dir.path(), &adapter));
        // The record is on a disk that hangs: the request gives up on it.
        let transport: Arc<dyn Transport> = Arc::new(ersa_adapter::LocalTransport);
        let found = |_: &str| Some(transport);
        let renamed = manager.set_title("s-gated", "Renamed", found).await;
        let timed_out = |e: &io::Error| e.kind() == io::ErrorKind::TimedOut;
        assert!(
            matches!(&renamed, Err(SessionError::Adapter(AdapterError::Io(e))) if timed_out(e)),
            "{renamed:?}"
        );

        // The rewrite is still running, so the session is still held: a
        // start asked for now waits, and never begins on a record that is
        // being written.
        let under_way = |manager: &SessionManager| {
            let state = manager.admission.snapshot();
            state.session("s-gated").unwrap().exclusive.clone()
        };
        assert_eq!(
            under_way(&manager),
            Some(crate::admission::Exclusive::Other)
        );
        let start = Command::BeginStart {
            session: "s-gated".into(),
        };
        let starting = tokio::spawn({
            let (admission, start) = (Arc::clone(&manager.admission), start.clone());
            async move { admission.command(start).await }
        });
        assert_eq!(held_in(&manager).await, [start]);
        assert!(!starting.is_finished());

        // The rewrite ends: the start has its turn.
        adapter.rename.notify_one();
        let started = tokio::time::timeout(Duration::from_secs(5), starting).await;
        assert!(started.expect("answered in time").expect("joined").is_ok());
    }

    /// What admission holds, once it holds something.
    async fn held_in(manager: &SessionManager) -> Vec<Command> {
        let waiting = async {
            loop {
                let state = manager.admission.snapshot();
                if !state.held().is_empty() {
                    return state.held().to_vec();
                }
                tokio::task::yield_now().await;
            }
        };
        let waited = tokio::time::timeout(Duration::from_secs(5), waiting).await;
        waited.expect("a command is held in time")
    }

    // verifies: PROTO-CORE-API-R39
    #[tokio::test]
    async fn an_arrangement_into_a_section_and_the_section_s_removal_wait_for_each_other() {
        let adapter = Arc::new(GatedReads::default());
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(gated_manager(dir.path(), &adapter));
        store_gated(&manager).await;
        let (name, key) = ("Later".to_string(), "later".to_string());
        let section = manager.store.insert_section(name, key).await.unwrap().id;
        manager.admission.report(Report::SectionStored {
            section: section.clone(),
        });
        let into = |manager: Arc<SessionManager>, section: String| {
            tokio::spawn(async move {
                let section = Some(Some(section));
                manager.arrange("s-gated", None, section).await
            })
        };

        // A removal of the section is under way: an arrangement into it
        // waits, named to admission with the section it moves into.
        let remove = Command::RemoveSection {
            section: section.clone(),
        };
        let removing = manager.admission.command(remove.clone()).await.unwrap();
        let arranging = into(Arc::clone(&manager), section.clone());
        let arrange = Command::Arrange {
            session: "s-gated".into(),
            section: Some(section.clone()),
        };
        let held = held_in(&manager).await;
        assert_eq!(held.as_slice(), std::slice::from_ref(&arrange));
        // The removal fails: the section stays, and the arrangement goes on.
        drop(removing);
        let arranged = arranging.await.unwrap().unwrap();
        assert_eq!(arranged.section_id.as_ref(), Some(&section));

        // An arrangement into it is under way: its removal waits, and then
        // takes the session out of the section with the rest.
        let arranging = manager.admission.command(arrange).await.unwrap();
        let removing = tokio::spawn({
            let (manager, section) = (Arc::clone(&manager), section.clone());
            async move { manager.remove_section(&section).await }
        });
        assert_eq!(held_in(&manager).await, [remove]);
        drop(arranging);
        let removed = removing.await.unwrap().unwrap();
        assert_eq!(removed.map(|removed| removed.id), Some(section.clone()));
        assert_eq!(manager.get("s-gated").unwrap().section_id, None);
        assert_eq!(manager.admission.snapshot().section(&section), None);
        let stored = manager.store.session("s-gated".into()).await.unwrap();
        assert_eq!(stored.and_then(|row| row.section_id), None);

        // No such section now, for an arrangement or a removal.
        let missing = into(Arc::clone(&manager), section.clone()).await.unwrap();
        assert!(
            matches!(missing, Err(SessionError::NoSection(_))),
            "{missing:?}"
        );
        assert!(manager.remove_section(&section).await.unwrap().is_none());
        // That the session is being deleted is said before that.
        manager.mark_deleting("s-gated", false).await.unwrap();
        let deleting = into(Arc::clone(&manager), section).await.unwrap();
        let reason = ErrorReason::SessionDeleting;
        assert!(
            matches!(&deleting, Err(SessionError::Refused { reason: r, .. }) if *r == reason),
            "{deleting:?}"
        );
    }

    // verifies: PROTO-CORE-API-R71
    #[tokio::test]
    async fn a_row_with_no_permission_mode_reads_as_its_backend_s_default() {
        let adapter = Arc::new(GatedReads::default());
        let dir = tempfile::tempdir().unwrap();
        let manager = gated_manager(dir.path(), &adapter);
        let row = manager.row("s-gated").unwrap();
        assert_eq!(row.permission_mode, None);
        // A session loaded from before Core recorded the mode, or found in
        // the backend's records: its handle knows the default.
        let admission = Arc::clone(&manager.admission);
        let default = Some("default".to_string());
        let handle = Handle::new(&row, now_rfc3339(), admission, default);
        assert_eq!(
            handle.row().unwrap().permission_mode.as_deref(),
            Some("default")
        );
        let listed = handle.describe().unwrap().permission_mode;
        assert_eq!(listed, "default");
        // What admission keeps is the stored row, and a stored value wins.
        assert_eq!(
            manager.admission.row("s-gated").unwrap().permission_mode,
            None
        );
        let change = RowChange::Requested {
            setting: crate::admission::Setting::PermissionMode,
            value: "plan".into(),
            at: now_rfc3339(),
        };
        assert!(manager.admission.wrote("s-gated", change));
        assert_eq!(
            handle.row().unwrap().permission_mode.as_deref(),
            Some("plan")
        );
    }

    #[tokio::test]
    async fn nothing_is_launched_once_core_is_shutting_down() {
        let adapter = Arc::new(GatedReads::default());
        let dir = tempfile::tempdir().unwrap();
        let manager = gated_manager(dir.path(), &adapter);
        manager.stop_all().await;
        // Admission was told, so the launch of a start that was under way
        // is refused, and the session is not live.
        assert!(matches!(
            manager.admission.launch("s-gated"),
            Err(Refusal::Closing)
        ));
        assert!(manager.handle("s-gated").unwrap().live().is_none());
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
            let (sink, finished, run) = make_live(&manager);
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
                drop(run);
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
            .insert_project(project, persistence::new_id("w"), "c:/work".into())
            .await
            .unwrap();
        let mut row = manager.row("s-gated").unwrap();
        row.workspace_id = workspace.id;
        store.insert_session(row).await.unwrap();
        let (sink, _finished, _run) = make_live(&manager);
        sink.start_finished(Ok(()));
        sink.process_exited(Ok(ProcessExit { code: Some(0) }));
        let removing = tokio::spawn({
            let manager = Arc::clone(&manager);
            async move { manager.remove("s-gated").await }
        });
        tokio::time::sleep(Duration::from_millis(1)).await;
        // While it waits, the session's other operations are not held up.
        let other = tokio::time::timeout(Duration::from_millis(1), manager.exclusive("s-gated"));
        assert!(matches!(other.await, Ok(Ok(_))), "held up behind the wait");
        // The removal is under way in admission all the same, so a delete
        // of the session asked for now waits for it.
        let waiting = manager.admission.snapshot();
        assert!(waiting.session("s-gated").is_some_and(|s| s.removing));
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
        let gone = || removed.load(Ordering::SeqCst);
        assert_eq!(read_unless_removed(async { 1 }, gone, forget).await, 1);
        assert!(!forgot.load(Ordering::SeqCst));
        // The session is removed while the read runs: what it kept goes.
        let read = async {
            removed.store(true, Ordering::SeqCst);
            2
        };
        assert_eq!(read_unless_removed(read, gone, forget).await, 2);
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

    /// A dormant session's facts outlive the log's rotation, and one a
    /// crashed Core left running is ended at the first restart, adapter or
    /// not, so its end is dated then.
    // verifies: PROTO-CORE-API-R67
    #[tokio::test]
    async fn a_dormant_sessions_facts_are_kept_through_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("ersa.db")).unwrap();
        store.ensure_local_host("pc".into()).await.unwrap();
        let project = ersa_protocol::core_api::Project {
            id: "p".into(),
            host_id: persistence::LOCAL_HOST_ID.into(),
            path: "C:/work".into(),
            name: "work".into(),
            vcs: ersa_protocol::core_api::Vcs::None,
            trusted: false,
        };
        let workspace = store
            .insert_project(project, persistence::new_id("w"), "c:/work".into())
            .await
            .unwrap();
        store
            .insert_session(SessionRow {
                id: "s-dormant".into(),
                workspace_id: workspace.id,
                host_id: persistence::LOCAL_HOST_ID.into(),
                backend: "gone".into(),
                backend_session_id: None,
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
                forked_from: None,
                forked_through: None,
                model_at: None,
                effort_at: None,
                permission_mode_at: None,
                started_elsewhere: false,
            })
            .await
            .unwrap();
        // A previous Core started its backend, then died.
        let (log, _) = AppLog::open(dir.path()).unwrap();
        let started = LifecycleFact::ProcessStarted {
            backend: "gone".into(),
            backend_version: "1.0".into(),
        };
        log.write("s-dormant", "2026-01-01T00:00:00.000Z", &started);
        log.flushed().await;
        drop(log);

        // This Core has no adapter for it.
        let (log, facts) = AppLog::open(dir.path()).unwrap();
        let log = Arc::new(log);
        let manager = SessionManager::new(store, Vec::new(), Arc::clone(&log), Arc::default());
        manager.recover(&facts, |_| None).await.unwrap();
        assert!(manager.list().is_empty());
        // Enough facts about another session to rotate the log twice over.
        let filler = LifecycleFact::StartFailed {
            message: "x".repeat(1024),
            code: None,
            stopped_by_user: false,
        };
        for _ in 0..(crate::app_log::ROTATE_BYTES as usize / 1024) * 2 + 10 {
            log.write("s-other", "2026-01-02T00:00:00.000Z", &filler);
        }
        log.flushed().await;
        drop(manager);
        drop(log);

        let (_, facts) = AppLog::open(dir.path()).unwrap();
        let kept = &facts["s-dormant"];
        assert_eq!(kept.first.ts, "2026-01-01T00:00:00.000Z");
        assert_eq!(
            kept.last.as_ref().unwrap().fact,
            LifecycleFact::CoreRestarted
        );
    }
}
