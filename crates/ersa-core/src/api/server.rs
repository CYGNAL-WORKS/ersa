//! The Core API server: listens on the platform endpoint and serves each
//! connection in its own task, speaking newline-delimited JSON-RPC 2.0.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use ersa_protocol::core_api::rpc::{Incoming, Response, parse_incoming};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use super::{After, ConnectionState};
use crate::persistence::{self, Store};
use crate::platform::{self, BoxStream, Listener};
use crate::state::{Core, CoreConfig};

/// How many outgoing lines may queue for one client before writers wait.
const OUTBOUND_QUEUE: usize = 256;

/// How long shutdown waits for connections to finish writing.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Why the server could not start.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("another Ersa Core is already serving on {endpoint}")]
    AlreadyRunning { endpoint: String },
    #[error("could not listen on {endpoint}: {source}")]
    Listen { endpoint: String, source: io::Error },
    #[error("could not create the data directory {path}: {source}")]
    DataDir { path: String, source: io::Error },
    #[error("could not open the database: {0}")]
    Database(#[from] persistence::Error),
}

/// A bound server, ready to [`run`](Server::run).
pub struct Server {
    core: Arc<Core>,
    listener: Listener,
}

impl Server {
    /// Claims the endpoint, then opens the database and runs its migrations.
    ///
    /// The endpoint comes first: if another Core already owns it, this one
    /// must not touch the database the other is using.
    pub async fn bind(config: CoreConfig) -> Result<Server, ServeError> {
        let listener = Listener::bind(&config.endpoint).map_err(|source| {
            if source.kind() == io::ErrorKind::AddrInUse {
                ServeError::AlreadyRunning {
                    endpoint: config.endpoint.clone(),
                }
            } else {
                ServeError::Listen {
                    endpoint: config.endpoint.clone(),
                    source,
                }
            }
        })?;

        std::fs::create_dir_all(&config.data_dir).map_err(|source| ServeError::DataDir {
            path: config.data_dir.display().to_string(),
            source,
        })?;
        let store = Store::open(&config.db_path())?;
        let core = Core::new(config, store);
        core.store
            .ensure_local_host(core.host_name().to_string())
            .await?;
        Ok(Server {
            core: Arc::new(core),
            listener,
        })
    }

    /// The shared state, for embedding Core in-process (tests).
    pub fn core(&self) -> Arc<Core> {
        Arc::clone(&self.core)
    }

    /// Accepts connections until `core.shutdown` is requested, then waits
    /// briefly for open connections to finish.
    pub async fn run(mut self) -> io::Result<()> {
        tracing::info!(endpoint = %self.core.config.endpoint, "serving");
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                accepted = self.listener.accept() => match accepted {
                    Ok(stream) => {
                        connections.spawn(serve_connection(Arc::clone(&self.core), stream));
                    }
                    // One failed accept (a client that vanished mid-connect)
                    // must not stop the server. The pause keeps a failure
                    // that repeats from turning into a busy loop.
                    Err(e) => {
                        tracing::warn!(error = %e, "accept failed");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
                _ = self.core.shutdown_requested() => break,
                // Reap finished connection tasks so the set does not grow.
                Some(_) = connections.join_next(), if !connections.is_empty() => {}
            }
        }
        // M0-T6: stop every session cleanly here, before the process exits.
        tracing::info!("shutting down");
        let drain = async { while connections.join_next().await.is_some() {} };
        if tokio::time::timeout(SHUTDOWN_GRACE, drain).await.is_err() {
            connections.abort_all();
        }
        Ok(())
    }
}

/// Serves one client until it disconnects, the handshake fails, or Core
/// shuts down. Requests are answered one at a time, in order.
async fn serve_connection(core: Arc<Core>, stream: BoxStream) {
    let (read, mut write) = tokio::io::split(stream);

    // One task owns the write half and writes whatever the channel delivers,
    // so responses and (from M0-T6) notifications never interleave mid-line.
    let (outbound, mut queue) = mpsc::channel::<String>(OUTBOUND_QUEUE);
    let writer = tokio::spawn(async move {
        while let Some(mut line) = queue.recv().await {
            line.push('\n');
            if let Err(e) = write.write_all(line.as_bytes()).await {
                tracing::debug!(error = %e, "client went away while writing");
                return;
            }
            if write.flush().await.is_err() {
                return;
            }
        }
        write.shutdown().await.ok();
    });

    let mut conn = ConnectionState {
        api_version: None,
        outbound,
    };
    let mut lines = BufReader::new(read).lines();
    let mut shutdown_after = false;
    loop {
        let line = tokio::select! {
            line = lines.next_line() => line,
            _ = core.shutdown_requested() => break,
        };
        let line = match line {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(e) => {
                tracing::debug!(error = %e, "reading from client failed");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        let (response, after) = match parse_incoming(&line) {
            Incoming::Request(request) => super::handle(&core, &mut conn, request).await,
            // The Interface sends no notifications yet; ignore any, as
            // JSON-RPC requires (a notification is never answered).
            Incoming::Notification { method, .. } => {
                tracing::debug!(%method, "ignoring notification from client");
                continue;
            }
            Incoming::Invalid { id, error } => (Response::failure(id, error), After::Continue),
        };
        if !send(&conn, &response).await {
            break;
        }
        match after {
            After::Continue => {}
            After::Close => break,
            After::Shutdown => {
                shutdown_after = true;
                break;
            }
        }
    }

    // Dropping the sender ends the writer once it has written everything
    // queued, including the answer to `core.shutdown`.
    drop(conn);
    writer.await.ok();
    if shutdown_after {
        core.request_shutdown();
    }
}

/// Queues `response` for writing. `false` when the writer has stopped.
async fn send(conn: &ConnectionState, response: &Response) -> bool {
    let line = serde_json::to_string(response).expect("responses always serialize");
    conn.outbound.send(line).await.is_ok()
}

/// Opens a client connection to a Core at `endpoint`. For tests and tools.
pub async fn connect(endpoint: &str) -> io::Result<BoxStream> {
    platform::connect(endpoint).await
}
