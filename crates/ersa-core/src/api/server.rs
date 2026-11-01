//! The Core API server: listens on the platform endpoint and serves each
//! connection in its own task, speaking newline-delimited JSON-RPC 2.0.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use ersa_protocol::core_api::rpc::{ErrorObject, Incoming, Response, code, parse_incoming};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use super::lines::{CappedLines, LineError};
use super::{After, ConnectionState};
use crate::persistence::{self, Store};
use crate::platform::{self, BoxStream, Listener};
use crate::state::{Core, CoreConfig};

/// How many outgoing lines may queue for one client before writers wait.
///
/// M0-T6: responses may wait for room, but `session.event` fan-out must not.
/// Use `try_send` for notifications and, when the queue is full, drop the
/// slow subscriber (or close its connection); never `.await` a send on
/// behalf of a session, or one stalled client stalls that session for every
/// other client.
const OUTBOUND_QUEUE: usize = 256;

/// How long shutdown waits for connections to finish writing.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Why the server could not start.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("another Ersa Core is already serving on {endpoint}")]
    AlreadyRunning { endpoint: String },
    /// The endpoint's name is held by a process that does not run as this
    /// user. Starting anyway would hand that process this user's clients.
    #[error("{endpoint} is held by a process running as another user: {source}")]
    NameTaken { endpoint: String, source: io::Error },
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
        let endpoint = config.endpoint.clone();
        let listener = Listener::bind(&endpoint)
            .await
            .map_err(|source| match source.kind() {
                io::ErrorKind::AddrInUse => ServeError::AlreadyRunning { endpoint },
                io::ErrorKind::PermissionDenied => ServeError::NameTaken { endpoint, source },
                _ => ServeError::Listen { endpoint, source },
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
        // A previous Core that stopped mid-create or mid-remove left rows
        // that no operation will ever finish. Mark them `error`, which
        // `workspace.remove` accepts, so the user can clear them.
        let stuck = core.store.reconcile_workspaces().await?;
        if stuck > 0 {
            tracing::warn!(stuck, "marked interrupted workspaces as error");
        }
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
    pub async fn run(self) -> io::Result<()> {
        let Server { core, mut listener } = self;
        tracing::info!(endpoint = %core.config.endpoint, "serving");
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => match accepted {
                    Ok(stream) => {
                        connections.spawn(serve_connection(Arc::clone(&core), stream));
                    }
                    // One failed accept (a client that vanished mid-connect)
                    // must not stop the server. The pause keeps a failure
                    // that repeats from turning into a busy loop.
                    Err(e) => {
                        tracing::warn!(error = %e, "accept failed");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
                _ = core.shutdown_requested() => break,
                // Reap finished connection tasks so the set does not grow.
                Some(_) = connections.join_next(), if !connections.is_empty() => {}
            }
        }
        // Release the endpoint first, so no new client connects to a Core
        // that is going away, and a new Core can start while this one drains.
        drop(listener);
        // M0-T6: stop every session cleanly here, before the process exits.
        tracing::info!("shutting down");
        let drain = async { while connections.join_next().await.is_some() {} };
        if tokio::time::timeout(SHUTDOWN_GRACE, drain).await.is_err() {
            connections.abort_all();
        }
        Ok(())
    }
}

/// Serves one client until it disconnects, the handshake fails or times out,
/// it sends an unreadable line, or Core shuts down. Requests are answered one
/// at a time, in order.
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
    let mut lines = CappedLines::new(read, core.config.max_line_bytes);
    let hello_deadline = tokio::time::Instant::now() + core.config.handshake_timeout;
    let mut shutdown_after = false;
    loop {
        // Before the handshake, a deadline also ends the wait: a client that
        // connects and says nothing must not hold a connection forever.
        let waiting_for_hello = conn.api_version.is_none();
        let line = tokio::select! {
            line = lines.next_line() => line,
            _ = core.shutdown_requested() => break,
            _ = tokio::time::sleep_until(hello_deadline), if waiting_for_hello => {
                tracing::debug!("closing a connection that sent no core.hello in time");
                break;
            }
        };
        let line = match line {
            Ok(Some(line)) => line,
            Ok(None) => break,
            // An over-long or non-UTF-8 line: there is no request to answer,
            // so the error has a null id, and the stream cannot be trusted
            // past it, so the connection closes.
            Err(LineError::TooLong) => {
                let message = format!("line longer than {} bytes", core.config.max_line_bytes);
                reject_stream(&conn, message).await;
                break;
            }
            Err(LineError::NotUtf8) => {
                reject_stream(&conn, "line is not UTF-8".into()).await;
                break;
            }
            Err(LineError::Io(e)) => {
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
        let sent = send(&conn, &response).await;
        // Decided before looking at `sent`: a client that vanished right
        // after asking for shutdown still gets Core shut down.
        if after == After::Shutdown {
            shutdown_after = true;
        }
        if !sent || after != After::Continue {
            break;
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

/// Answers an unreadable line with a parse error.
async fn reject_stream(conn: &ConnectionState, message: String) {
    let error = ErrorObject::protocol(code::PARSE_ERROR, message);
    send(conn, &Response::failure(None, error)).await;
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
