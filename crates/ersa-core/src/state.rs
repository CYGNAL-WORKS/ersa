//! The state every request handler shares: the database, the transports, and
//! what Core knows about its own host.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ersa_adapter::{Adapter, Transport};
use ersa_protocol::core_api::{BackendState, HelloHost};
use tokio::sync::{Mutex, watch};

use crate::adapters::{self, Detected};
use crate::app_log::AppLog;
use crate::persistence::{LOCAL_HOST_ID, Store};
use crate::platform;
use crate::sessions::SessionManager;
use crate::transport::LocalTreeTransport;

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
    /// How long a backend detection that found anything other than
    /// `available` is reused. [`FAILED_DETECTION_TTL`], except in tests
    /// that need every detection after a failed one to run again.
    pub failed_detection_ttl: Duration,
}

/// The default for [`CoreConfig::max_line_bytes`]: 16 MiB, far above any
/// request the Interface sends, low enough that one client cannot exhaust
/// memory.
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// The default for [`CoreConfig::handshake_timeout`].
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a backend detection that found anything other than `available`
/// is reused, so a backend installed while Core runs shows up soon after.
pub const FAILED_DETECTION_TTL: Duration = Duration::from_secs(30);

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
            failed_detection_ttl: FAILED_DETECTION_TTL,
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

/// Core's shared state. Handlers receive `&Core`; the server holds it in an
/// `Arc` so every connection task can.
pub struct Core {
    pub config: CoreConfig,
    pub store: Store,
    /// Every session, and the adapters that start them.
    pub sessions: SessionManager,
    local_transport: Arc<dyn Transport>,
    /// The local transport for backends, whose leftovers die with them.
    local_session_transport: Arc<dyn Transport>,
    host_name: String,
    started: Instant,
    /// The last backend detection for the local host and when it ran. Kept
    /// for the life of the process when every backend was available, and for
    /// [`FAILED_DETECTION_TTL`] otherwise. An async mutex, held while
    /// detecting, so concurrent requests run one detection between them.
    local_backends: Mutex<Option<(Instant, Vec<Detected>)>>,
    /// Flips to `true` once when shutdown is requested. A `watch` channel
    /// rather than a `Notify`, because a late waiter must still see it.
    shutdown: watch::Sender<bool>,
    /// For a replay server: rises once a client first subscribes, which is
    /// when the replay starts playing (see `replay`).
    replay_audience: watch::Sender<bool>,
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
        let sessions = SessionManager::new(store.clone(), adapters, app_log)
            .with_copy_pause(config.copy_pause)
            .with_wind_down_pause(config.wind_down_pause);
        Core {
            config,
            sessions,
            store,
            local_transport: Arc::new(LocalTreeTransport::for_commands()),
            local_session_transport: Arc::new(LocalTreeTransport::new()),
            host_name: platform::host_name(),
            started: Instant::now(),
            local_backends: Mutex::new(None),
            shutdown: watch::Sender::new(false),
            replay_audience: watch::Sender::new(false),
        }
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

    /// The backends detected on the local host, detecting again when the
    /// last result has expired.
    pub async fn local_backends(&self) -> Vec<Detected> {
        let mut cached = self.local_backends.lock().await;
        if let Some((at, found)) = cached.as_ref() {
            let all_available = found.iter().all(|d| d.state == BackendState::Available);
            if all_available || at.elapsed() < self.config.failed_detection_ttl {
                return found.clone();
            }
        }
        let found = adapters::detect(
            self.local_transport.as_ref(),
            Arc::clone(&self.local_session_transport),
            self.config.backend_program_override.as_deref(),
            self.sessions.adapters(),
        )
        .await;
        *cached = Some((Instant::now(), found.clone()));
        found
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
