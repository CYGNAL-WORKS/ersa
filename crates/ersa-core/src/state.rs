//! The state every request handler shares: the database, the transports, and
//! what Core knows about its own host.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use ersa_adapter::{LocalTransport, Transport};
use ersa_protocol::core_api::HelloHost;
use tokio::sync::{OnceCell, watch};

use crate::adapters::{self, Detected};
use crate::persistence::{LOCAL_HOST_ID, Store};
use crate::platform;

/// How a Core process is set up. Built by the `ersa-core` binary from its
/// command line, and by tests with a temporary data directory and a unique
/// endpoint.
#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// Where the database (and, by default, worktrees) live.
    pub data_dir: PathBuf,
    /// The named pipe or socket path to listen on.
    pub endpoint: String,
    /// Runs this program instead of each backend's own when detecting
    /// backends. For tests, which must not depend on what is installed.
    pub backend_program_override: Option<String>,
}

impl CoreConfig {
    /// The configuration for `data_dir` with the platform's default endpoint.
    pub fn new(data_dir: PathBuf) -> Self {
        CoreConfig {
            data_dir,
            endpoint: platform::default_endpoint(),
            backend_program_override: None,
        }
    }

    /// The database file.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("ersa.db")
    }
}

/// Core's shared state. Handlers receive `&Core`; the server holds it in an
/// `Arc` so every connection task can.
///
/// The session manager (M0-T6) belongs here as another field, built in
/// `Core::new` from the store and the transports.
pub struct Core {
    pub config: CoreConfig,
    pub store: Store,
    local_transport: Arc<dyn Transport>,
    host_name: String,
    started: Instant,
    /// Backend detection for the local host, run on first use and kept for
    /// the life of the process (`host.probe`, milestone M4, will refresh it).
    local_backends: OnceCell<Vec<Detected>>,
    /// Flips to `true` once when shutdown is requested. A `watch` channel
    /// rather than a `Notify`, because a late waiter must still see it.
    shutdown: watch::Sender<bool>,
}

impl Core {
    pub fn new(config: CoreConfig, store: Store) -> Core {
        Core {
            config,
            store,
            local_transport: Arc::new(LocalTransport),
            host_name: platform::host_name(),
            started: Instant::now(),
            local_backends: OnceCell::new(),
            shutdown: watch::Sender::new(false),
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

    /// The backends detected on the local host.
    pub async fn local_backends(&self) -> &[Detected] {
        self.local_backends
            .get_or_init(|| async {
                adapters::detect(
                    self.local_transport.as_ref(),
                    self.config.backend_program_override.as_deref(),
                )
                .await
            })
            .await
    }

    /// Seconds since this Core started.
    pub fn uptime_s(&self) -> u64 {
        self.started.elapsed().as_secs()
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
