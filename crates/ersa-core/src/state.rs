//! The state every request handler shares: the database, the transports, and
//! what Core knows about its own host.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ersa_adapter::{Adapter, Transport};
use ersa_protocol::core_api::{BackendState, HelloHost};
use tokio::sync::{Mutex, Semaphore, watch};

use crate::adapters::{self, Detected};
use crate::admission::Admission;
use crate::app_log::AppLog;
use crate::persistence::{LOCAL_HOST_ID, Store};
use crate::platform;
use crate::search_path::{StoredEnvironment, SystemStoredEnvironment};
use crate::sessions::SessionManager;
use crate::transport::{Finder, LOOK_LIMIT, LOOKS, LocalTreeTransport};

/// How a Core process is set up. Built by the `ersa-core` binary from its
/// command line, and by tests with a temporary data directory and a unique
/// endpoint.
#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// Where the database, the app log and (by default) worktrees live.
    pub data_dir: PathBuf,
    /// The named pipe or socket path to listen on.
    pub endpoint: String,
    /// Runs this program instead of each backend's own when detecting
    /// backends. For tests, which must not depend on what is installed.
    pub backend_program_override: Option<String>,
    /// How long a new connection may take to send `core.hello` before Core
    /// closes it.
    pub handshake_timeout: Duration,
    /// The longest line Core reads from a client, in bytes, terminator
    /// excluded. A longer line is answered with a parse error and closes
    /// the connection.
    pub max_line_bytes: usize,
    /// Whether this Core is a replay server, which refuses every request
    /// that would change anything (see `replay`). Set by
    /// `Server::bind_replay`.
    pub replay: bool,
    /// A pause after each chunk an attachment's copy writes. Zero, except
    /// in tests that need a copy to be under way long enough to cancel it.
    pub copy_pause: Duration,
    /// A pause between a session's end being announced and Core letting go
    /// of its run. Zero, except in tests that need a request to land while
    /// a run winds down.
    pub wind_down_pause: Duration,
    /// Where the folder inspection of `project.add` and `project.find`, and
    /// each try at a workspace's folder (every removal, PROTO-CORE-API-R122), stops, on its
    /// own thread before it touches the folder, until a test lets it go: a
    /// folder that does not answer. `None`, except in tests.
    pub folder_gate: Option<Arc<Gate>>,
    /// How long the folder inspection of `project.add` and `project.find`
    /// may take before they answer `permission` (PROTO-CORE-API-R93).
    /// [`FOLDER_INSPECTION_LIMIT`], except in tests that let it run out.
    pub folder_inspection_limit: Duration,
    /// How long `session.delete` waits for the adapter to delete the
    /// session's record before it gives up and calls the deletion off.
    /// [`RECORD_DELETION_LIMIT`], except in tests that let it run out.
    pub record_deletion_limit: Duration,
    /// How long each try at a workspace's folder (every removal) may
    /// take before it gives up and calls the try off.
    /// [`crate::workspace::GIT_TIMEOUT`], as a git call has, except in tests
    /// that let it run out.
    pub folder_removal_limit: Duration,
    /// How long a removal goes on trying a workspace's folder that fails
    /// to delete: no try begins later than this after the first
    /// (PROTO-CORE-API-R113). [`crate::workspace::FOLDER_TRY_WINDOW`],
    /// except in tests that let it run out.
    pub folder_try_window: Duration,
    /// How long a request waits for its turn at a project's worktrees
    /// while another change to them is under way (PROTO-CORE-API-R119).
    /// [`crate::workspace::WORKTREE_TURN_WAIT`], except in tests that let
    /// it run out.
    pub worktree_turn_wait: Duration,
    /// The transport git and backends' version commands run through on the
    /// local host. `None` is the local machine's; tests put one in front
    /// of it that holds a command, as a slow repository or backend would.
    pub command_transport: Option<CommandTransport>,
    /// How long a backend detection that found the backend in `error` is
    /// reused. [`FAILED_DETECTION_TTL`], except in tests that need every
    /// detection after a failed one to run again. An `available` backend is
    /// kept for the life of the process and a `missing` one not at all
    /// (PROTO-CORE-API-R96, R149, R150).
    pub failed_detection_ttl: Duration,
    /// Where Core reads the search path as the system holds it now, to look
    /// for a backend's command (PROTO-CORE-API-R148): the registry on
    /// Windows, nothing elsewhere. Tests give their own.
    pub stored_environment: StoredEnvironmentSource,
    /// How long a look for a backend's command may take before the backend
    /// is `error` (PROTO-CORE-API-R151). [`LOOK_LIMIT`], except in tests
    /// that let it run out.
    pub backend_look_limit: Duration,
    /// When Core sweeps (FLOW-CLEAN-UP-R06). [`Sweeps::Hourly`], except in
    /// tests.
    pub sweeps: Sweeps,
    /// How long a backend's sign-in may take to start (PROTO-CORE-API-R157).
    /// [`crate::sign_in::SIGN_IN_START_LIMIT`], except in tests that let it
    /// run out.
    pub sign_in_start_limit: Duration,
    /// How long a backend's sign-in may run once started
    /// (PROTO-CORE-API-R161). [`crate::sign_in::SIGN_IN_LIMIT`], except in
    /// tests that let it run out.
    pub sign_in_limit: Duration,
}

/// A source for [`CoreConfig::stored_environment`]. Its own type only so
/// that `CoreConfig` can derive `Debug`.
#[derive(Clone)]
pub struct StoredEnvironmentSource(pub Arc<dyn StoredEnvironment>);

impl std::fmt::Debug for StoredEnvironmentSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StoredEnvironmentSource")
    }
}

/// When Core sweeps ([`CoreConfig::sweeps`]).
#[derive(Debug, Clone)]
pub enum Sweeps {
    /// When Core starts, every [`SWEEP_INTERVAL`] after that, and after a
    /// session is archived (FLOW-CLEAN-UP-R06).
    Hourly,
    /// As [`Sweeps::Hourly`], except that the test says when an hour has
    /// passed, and can wait for each sweep to end.
    Driven(Arc<SweepClock>),
    /// Never: for tests that do not look at the sweep, so that archiving a
    /// session changes nothing else under them.
    Never,
}

/// How often Core sweeps after it starts: at least once an hour
/// (FLOW-CLEAN-UP-R06).
pub const SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// The sweep's clock in a test ([`Sweeps::Driven`]): the test says when an
/// hour has passed, and learns when each sweep has ended.
#[derive(Debug)]
pub struct SweepClock {
    /// One permit for each hour the test said has passed and the sweep has
    /// not taken yet.
    hours: tokio::sync::Semaphore,
    /// How many sweeps have ended.
    ended: watch::Sender<u64>,
}

impl Default for SweepClock {
    fn default() -> Self {
        SweepClock {
            hours: tokio::sync::Semaphore::new(0),
            ended: watch::Sender::new(0),
        }
    }
}

impl SweepClock {
    /// Says that an hour has passed: the sweep runs again.
    pub fn hour_passed(&self) {
        self.hours.add_permits(1);
    }

    /// How many sweeps have ended so far.
    pub fn ended(&self) -> u64 {
        *self.ended.borrow()
    }

    /// Waits until `count` sweeps have ended.
    pub async fn until_ended(&self, count: u64) {
        let mut ended = self.ended.subscribe();
        // The sender lives in `self`, so the wait cannot fail.
        let _ = ended.wait_for(|ended| *ended >= count).await;
    }

    /// Waits until the test says an hour has passed.
    pub(crate) async fn hour(&self) {
        // The semaphore is never closed, so acquiring cannot fail.
        if let Ok(permit) = self.hours.acquire().await {
            permit.forget();
        }
    }

    /// Counts a sweep that ended.
    pub(crate) fn sweep_ended(&self) {
        self.ended.send_modify(|ended| *ended += 1);
    }
}

/// A transport for [`CoreConfig::command_transport`]. Its own type only so
/// that `CoreConfig` can derive `Debug`, which a transport has none of.
#[derive(Clone)]
pub struct CommandTransport(pub Arc<dyn Transport>);

impl std::fmt::Debug for CommandTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CommandTransport")
    }
}

/// A point a request stops at in tests until the test lets it go
/// ([`CoreConfig::folder_gate`]). Each stop is one [`Gate::reached`]
/// for the test and one [`Gate::go`] from it, in order.
#[derive(Debug)]
pub struct Gate {
    reached: tokio::sync::Semaphore,
    go: tokio::sync::Semaphore,
}

impl Default for Gate {
    /// Closed: the request stops at the first point it reaches.
    fn default() -> Self {
        Gate {
            reached: tokio::sync::Semaphore::new(0),
            go: tokio::sync::Semaphore::new(0),
        }
    }
}

impl Gate {
    /// Called by the request: says it got here, then waits to be let go.
    pub async fn stop(&self) {
        self.reached.add_permits(1);
        // The semaphore is never closed, so acquiring cannot fail.
        if let Ok(permit) = self.go.acquire().await {
            permit.forget();
        }
    }

    /// Waits until the request has stopped at the next point.
    pub async fn reached(&self) {
        if let Ok(permit) = self.reached.acquire().await {
            permit.forget();
        }
    }

    /// Lets the request go on from where it stopped.
    pub fn go(&self) {
        self.go.add_permits(1);
    }
}

/// The default for [`CoreConfig::max_line_bytes`]: 16 MiB, far above any
/// request the Interface sends, low enough that one client cannot exhaust
/// memory.
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// The default for [`CoreConfig::handshake_timeout`].
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a backend detection that found the backend in `error` is
/// reused before the next request that needs it detects it again.
pub const FAILED_DETECTION_TTL: Duration = Duration::from_secs(30);

/// How many folder inspections may be under way at once, the ones left
/// running past their limit included: as many as `attachment.inspect`
/// reads may hold.
pub const FOLDER_INSPECTIONS: usize = 8;

/// The default for [`CoreConfig::folder_inspection_limit`]: a folder on a
/// share that stopped answering never answers (PROTO-CORE-API-R93).
pub const FOLDER_INSPECTION_LIMIT: Duration = Duration::from_secs(10);

/// The default for [`CoreConfig::record_deletion_limit`]: step 6 of
/// `session.delete` has 30 seconds (PROTO-CORE-API, `session.delete`).
pub const RECORD_DELETION_LIMIT: Duration = Duration::from_secs(30);

impl CoreConfig {
    /// The configuration for `data_dir` with the platform's default endpoint
    /// and limits. Fails only if the endpoint cannot be named (on Windows, if
    /// the user's SID cannot be read).
    pub fn new(data_dir: PathBuf) -> std::io::Result<Self> {
        Ok(CoreConfig {
            data_dir,
            endpoint: platform::default_endpoint()?,
            backend_program_override: None,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            max_line_bytes: MAX_LINE_BYTES,
            replay: false,
            copy_pause: Duration::ZERO,
            wind_down_pause: Duration::ZERO,
            folder_gate: None,
            folder_inspection_limit: FOLDER_INSPECTION_LIMIT,
            record_deletion_limit: RECORD_DELETION_LIMIT,
            folder_removal_limit: crate::workspace::GIT_TIMEOUT,
            folder_try_window: crate::workspace::FOLDER_TRY_WINDOW,
            worktree_turn_wait: crate::workspace::WORKTREE_TURN_WAIT,
            command_transport: None,
            failed_detection_ttl: FAILED_DETECTION_TTL,
            stored_environment: StoredEnvironmentSource(Arc::new(SystemStoredEnvironment)),
            backend_look_limit: LOOK_LIMIT,
            sweeps: Sweeps::Hourly,
            sign_in_start_limit: crate::sign_in::SIGN_IN_START_LIMIT,
            sign_in_limit: crate::sign_in::SIGN_IN_LIMIT,
        })
    }

    /// The database file.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("ersa.db")
    }

    /// Where Core's app log is kept: beside the database.
    pub fn app_log_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }
}

/// What a folder inspection under way will answer: `None` until it does.
/// Every request waiting on it holds a receiver; one that stops waiting
/// leaves the inspection to the others.
pub(crate) type Inspection<T> = watch::Receiver<Option<Result<T, crate::api::ApiError>>>;

/// `project.find`'s inspections under way, by host and path as asked.
pub(crate) type Finding = HashMap<(String, String), Inspection<Option<String>>>;

/// Core's shared state. Handlers receive `&Core`; the server holds it in an
/// `Arc` so every connection task can.
pub struct Core {
    pub config: CoreConfig,
    pub store: Store,
    /// Decides every change to which projects, workspaces and sessions
    /// exist (ARCH-CORE, Admission).
    pub admission: Arc<Admission>,
    /// Every session, and the adapters that start them.
    pub sessions: SessionManager,
    local_transport: Arc<dyn Transport>,
    /// The local transport backends' version commands run through: the
    /// command transport, behind a [`Finder`] that looks for the command
    /// along the search path as it is now (PROTO-CORE-API-R148).
    local_command_transport: Arc<dyn Transport>,
    /// The local transport for backends, whose leftovers die with them,
    /// behind a [`Finder`] as well.
    local_session_transport: Arc<dyn Transport>,
    host_name: String,
    started: Instant,
    /// The last detection of each backend on the local host and when it
    /// ran, by the adapter's place in registration order (see
    /// [`Core::local_backends`]). An async mutex, held while detecting, so
    /// concurrent requests run one detection between them.
    local_backends: Mutex<Vec<Option<(Instant, Detected)>>>,
    /// The places for folder inspections (`project.add`, `project.find`).
    /// Each holds one on its thread until its file system call returns,
    /// past its time limit too, so folders that never answer take at most
    /// [`FOLDER_INSPECTIONS`] threads, never more.
    pub(crate) folder_inspections: Arc<Semaphore>,
    /// The inspections `project.find` has under way, by host and path as
    /// asked, each kept until its file system call returns: a find of the
    /// same folder meanwhile joins it rather than take another place.
    pub(crate) finding: Arc<std::sync::Mutex<Finding>>,
    /// Flips to `true` once when shutdown is requested. A `watch` channel
    /// rather than a `Notify`, because a late waiter must still see it.
    shutdown: watch::Sender<bool>,
    /// For a replay server: rises once a client first subscribes, which is
    /// when the replay starts playing (see `replay`).
    replay_audience: watch::Sender<bool>,
    /// Asks the sweep for one more pass, after a session is archived
    /// (FLOW-CLEAN-UP-R06). Asked while a pass runs, the next pass follows
    /// it; asked many times meanwhile, one does.
    sweep_asked: tokio::sync::Notify,
}

impl Core {
    /// Core's state over `store`, able to start sessions for the backends
    /// `adapters` drive.
    pub fn new(
        config: CoreConfig,
        store: Store,
        adapters: Vec<Arc<dyn Adapter>>,
        app_log: Arc<AppLog>,
    ) -> Core {
        let local_transport = config
            .command_transport
            .as_ref()
            .map(|CommandTransport(transport)| Arc::clone(transport))
            .unwrap_or_else(|| Arc::new(LocalTreeTransport::for_commands()));
        // Both backend transports share one set of places for looks, so a
        // share that stopped answering holds at most `LOOKS` threads.
        let looks = Arc::new(Semaphore::new(LOOKS));
        let finder = |inner: Arc<dyn Transport>| -> Arc<dyn Transport> {
            Arc::new(Finder::new(
                inner,
                Arc::clone(&config.stored_environment.0),
                config.backend_look_limit,
                Arc::clone(&looks),
            ))
        };
        let local_command_transport = finder(Arc::clone(&local_transport));
        let local_session_transport = finder(Arc::new(LocalTreeTransport::new()));
        let admission = Arc::new(Admission::default());
        let sessions =
            SessionManager::new(store.clone(), adapters, app_log, Arc::clone(&admission))
                .with_copy_pause(config.copy_pause)
                .with_wind_down_pause(config.wind_down_pause);
        Core {
            config,
            admission,
            sessions,
            store,
            local_transport,
            local_command_transport,
            local_session_transport,
            host_name: platform::host_name(),
            started: Instant::now(),
            local_backends: Mutex::new(Vec::new()),
            folder_inspections: Arc::new(Semaphore::new(FOLDER_INSPECTIONS)),
            finding: Arc::default(),
            shutdown: watch::Sender::new(false),
            replay_audience: watch::Sender::new(false),
            sweep_asked: tokio::sync::Notify::new(),
        }
    }

    /// Asks for a sweep once the one under way, if any, has ended.
    pub(crate) fn ask_for_sweep(&self) {
        // `notify_one` keeps one permit when nobody waits yet, so a sweep
        // asked for during a pass runs after it, and only once.
        self.sweep_asked.notify_one();
    }

    /// Waits until a sweep is asked for.
    pub(crate) async fn sweep_asked(&self) {
        self.sweep_asked.notified().await;
    }

    /// The local host's name.
    pub fn host_name(&self) -> &str {
        &self.host_name
    }

    /// The local host as the handshake reports it.
    pub fn hello_host(&self) -> HelloHost {
        HelloHost {
            id: LOCAL_HOST_ID.to_string(),
            name: self.host_name.clone(),
            platform: platform_name().to_string(),
        }
    }

    /// The transport that reaches `host_id`, or `None` for a host Core
    /// cannot reach. Only the local host exists until remote hosts (M4) add
    /// an SSH transport here.
    pub fn transport_for(&self, host_id: &str) -> Option<Arc<dyn Transport>> {
        (host_id == LOCAL_HOST_ID).then(|| Arc::clone(&self.local_transport))
    }

    /// The transport backends on `host_id` are started through: like
    /// [`Core::transport_for`], except that everything a backend started is
    /// killed when its session ends.
    pub fn session_transport_for(&self, host_id: &str) -> Option<Arc<dyn Transport>> {
        (host_id == LOCAL_HOST_ID).then(|| Arc::clone(&self.local_session_transport))
    }

    /// The backends detected on the local host, in the order the adapters
    /// were registered. Each backend's detection is kept apart from the
    /// others': an `available` one for the life of the process, so a
    /// missing one never has it started again (PROTO-CORE-API-R96, R149);
    /// a `missing` one not at all, since looking for its command starts
    /// nothing (R150); one in `error` for
    /// [`CoreConfig::failed_detection_ttl`]; a `signed_out` one not at all,
    /// so the backend is asked again at every reading (R153), which starts
    /// its command each time while it stays signed out.
    pub async fn local_backends(&self) -> Vec<Detected> {
        self.detect_local(None).await
    }

    /// Detects backend `name` on the local host again, whatever Core kept
    /// of it, and keeps what it finds as any detection is kept: before a
    /// sign-in is said to have ended signed in (PROTO-CORE-API-R159).
    pub async fn detect_again(&self, name: &str) {
        self.detect_local(Some(name)).await;
    }

    /// [`Core::local_backends`], detecting backend `again` whatever was
    /// kept of it.
    async fn detect_local(&self, again: Option<&str>) -> Vec<Detected> {
        let mut kept = self.local_backends.lock().await;
        let adapters = self.sessions.adapters();
        kept.resize(adapters.len(), None);
        let ttl = self.config.failed_detection_ttl;
        let stale: Vec<usize> = (0..adapters.len())
            .filter(|&i| {
                again == Some(adapters[i].backend())
                    || match &kept[i] {
                        None => true,
                        Some((at, detected)) => match detected.state {
                            BackendState::Available => false,
                            BackendState::Missing => true,
                            BackendState::Error => at.elapsed() >= ttl,
                            // Whether a backend is signed out is the backend's to say
                            // each time: Core keeps no copy (PROTO-CORE-API-R153).
                            BackendState::SignedOut => true,
                        },
                    }
            })
            .collect();
        if !stale.is_empty() {
            let to_detect: Vec<Arc<dyn Adapter>> =
                stale.iter().map(|&i| Arc::clone(&adapters[i])).collect();
            let found = adapters::detect(
                self.local_command_transport.as_ref(),
                Arc::clone(&self.local_session_transport),
                self.config.backend_program_override.as_deref(),
                &to_detect,
            )
            .await;
            let now = Instant::now();
            for (i, detected) in stale.into_iter().zip(found) {
                kept[i] = Some((now, detected));
            }
        }
        kept.iter().flatten().map(|(_, d)| d.clone()).collect()
    }

    /// Seconds since this Core started.
    pub fn uptime_s(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Whether a client has subscribed to the replayed session yet, for the
    /// replay's adapter.
    pub(crate) fn replay_audience(&self) -> watch::Receiver<bool> {
        self.replay_audience.subscribe()
    }

    pub(crate) fn replay_audience_sender(&self) -> &watch::Sender<bool> {
        &self.replay_audience
    }

    /// Asks the server to stop.
    pub fn request_shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    /// Whether shutdown has been requested.
    pub(crate) fn shutting_down(&self) -> bool {
        *self.shutdown.borrow()
    }

    /// Completes once shutdown has been requested, even if that was earlier.
    pub async fn shutdown_requested(&self) {
        let mut receiver = self.shutdown.subscribe();
        // `wait_for` checks the current value first, so an earlier request
        // counts. It fails only if the sender is gone, which it cannot be
        // while `self` is alive.
        let _ = receiver.wait_for(|requested| *requested).await;
    }
}

/// The operating system's name as `core.hello` reports it.
fn platform_name() -> &'static str {
    // `std::env::consts::OS` is `"windows"`, `"macos"`, `"linux"`, and so on.
    std::env::consts::OS
}
