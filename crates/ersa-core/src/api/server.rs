//! The Core API server: listens on the platform endpoint and serves each
//! connection in its own task, speaking newline-delimited JSON-RPC 2.0.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::Adapter;
use ersa_protocol::core_api::rpc::{ErrorObject, Incoming, Response, code, parse_incoming};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Semaphore, mpsc, watch};
use tokio::task::JoinSet;

use super::lines::{CappedLines, LineError};
use super::{After, ConnectionState};
use crate::persistence::{self, Store};
use crate::platform::{self, BoxStream, Listener};
use crate::state::{Core, CoreConfig};

/// How many outgoing lines may queue for one client before writers wait.
///
/// Responses, each subscription's forwarding task, and the tile forwarder
/// may wait for room; a session never does. The session manager offers
/// events to a subscription's own queue and drops a subscriber that falls
/// behind, and tiles reach a connection through a broadcast that drops a
/// lagging receiver: one stalled client must not stall a session for every
/// other client.
const OUTBOUND_QUEUE: usize = 256;

/// How many detached requests one connection may have in flight, running or
/// waiting on their session's lane. Past it the connection reads no further
/// requests until one finishes: the bound on what one client can queue.
const QUEUED_PER_CONNECTION: usize = 1024;

/// How long a `session.stop` waits for the requests queued before it on its
/// session before it starts stopping anyway.
const STOP_OVERTAKE: Duration = Duration::from_secs(5);

/// After Core decides to close a connection, how long it still tries to
/// write what is queued, the response to the request in hand included.
const KICK_GRACE: Duration = Duration::from_secs(5);

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
        Server::bind_with(config, Vec::new()).await
    }

    /// [`Server::bind`], with the adapters sessions can be started with.
    /// The `ersa-core` binary registers every backend Ersa supports.
    pub async fn bind_with(
        config: CoreConfig,
        adapters: Vec<Arc<dyn Adapter>>,
    ) -> Result<Server, ServeError> {
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
        let core = Core::new(config, store, adapters);
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
        // Sessions the previous Core left running are ended, and stale
        // projections re-derived, before any client can see them.
        core.sessions.recover().await?;
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
        // Stop every session cleanly before the process exits: an Interface
        // closing never stops sessions, but `core.shutdown` does (ADR 0011).
        tracing::info!("shutting down");
        core.sessions.stop_all().await;
        let drain = async { while connections.join_next().await.is_some() {} };
        if tokio::time::timeout(SHUTDOWN_GRACE, drain).await.is_err() {
            connections.abort_all();
        }
        Ok(())
    }
}

/// Serves one client until it disconnects, the handshake fails or times out,
/// it sends an unreadable line, or Core shuts down.
///
/// Requests are read in order. Most are answered in line, one at a time.
/// Session requests that act on a session run detached, each in a task of
/// its own, and answer when done, possibly out of order (responses carry
/// their request's id). Detached requests about the same session still run
/// in the order they were sent: each waits for the one before it in its
/// session's lane. Requests about different sessions overlap; the in-line
/// ones name no session and never wait for the detached ones. A `session.stop` whose
/// lane does not drain within a grace starts stopping anyway, ending the
/// requests stuck ahead of it; its answer still keeps its place.
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

    let kick = Arc::new(watch::Sender::new(false));
    let mut kicked = kick.subscribe();
    let mut conn = ConnectionState::new(outbound, kick);
    let mut lines = CappedLines::new(read, core.config.max_line_bytes);
    let queued = Arc::new(Semaphore::new(QUEUED_PER_CONNECTION));
    // Per session: a flag that rises when the latest request about it has
    // been answered. The next request about that session waits for it.
    let mut lanes: HashMap<String, watch::Receiver<bool>> = HashMap::new();
    let hello_deadline = tokio::time::Instant::now() + core.config.handshake_timeout;
    let mut shutdown_after = false;
    loop {
        // Before the handshake, a deadline also ends the wait: a client that
        // connects and says nothing must not hold a connection forever.
        let waiting_for_hello = conn.api_version.is_none();
        let line = tokio::select! {
            line = lines.next_line() => line,
            _ = core.shutdown_requested() => break,
            // Core closes a connection that cannot keep up with its
            // notifications; its client reconnects and resubscribes.
            _ = kicked.wait_for(|kicked| *kicked) => break,
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
            // A session request that may wait on a backend runs on its own
            // and answers through the queue when done, so it never holds up
            // the requests behind it: a permission decision must not wait
            // for another session's start.
            Incoming::Request(request) if super::runs_detached(&conn, &request) => {
                // A client with too much in flight stops being read, which is
                // the back-pressure; a kick or shutdown still ends the wait.
                let in_flight = tokio::select! {
                    permit = Arc::clone(&queued).acquire_owned() => match permit {
                        Ok(permit) => permit,
                        Err(_) => break,
                    },
                    () = async { let _ = kicked.wait_for(|kicked| *kicked).await; } => break,
                    _ = core.shutdown_requested() => break,
                };
                let stops = super::stop_target(&request);
                let (done, done_rx) = watch::channel(false);
                let before = super::session_lane(&conn, &request)
                    .and_then(|session| lanes.insert(session, done_rx));
                lanes.retain(|_, answered| !*answered.borrow());
                // Weak, so a request still running does not keep a closed
                // connection's writer alive; its answer then goes nowhere.
                let peer = conn.peer();
                let core = Arc::clone(&core);
                let lane = super::session_lane(&conn, &request);
                tokio::spawn(async move {
                    if let Some(mut before) = before {
                        // `Err` from `wait_for` means the earlier task is
                        // gone: done too. A stop keeps its place behind the
                        // requests sent before it while they move, but not
                        // behind ones stuck on a start that hangs or a
                        // backend that stopped reading: after a grace it
                        // starts stopping, which ends them, and its answer
                        // still comes after theirs.
                        let waited = async {
                            let _ = before.wait_for(|answered| *answered).await;
                        };
                        tokio::pin!(waited);
                        let mut finished = false;
                        if let Some(session) = &stops {
                            // A completed future must not be polled again.
                            finished = tokio::time::timeout(STOP_OVERTAKE, &mut waited)
                                .await
                                .is_ok();
                            if !finished {
                                core.sessions.request_stop(session);
                            }
                        }
                        if !finished {
                            waited.await;
                        }
                    }
                    let method = request.method.clone();
                    let response = run_detached(core, request, peer.clone()).await;
                    let line =
                        serde_json::to_string(&response).expect("responses always serialize");
                    if let Some(outbound) = peer.outbound.upgrade() {
                        let _ = outbound.send(line).await;
                    }
                    peer.after_answer(&method, lane.as_deref());
                    done.send_replace(true);
                    drop(in_flight);
                });
                continue;
            }
            // In line: these never wait on a backend or on the detached
            // requests, so reading carries on behind them.
            Incoming::Request(request) => super::handle(&core, &mut conn, request).await,
            // The Interface sends no notifications yet; ignore any, as
            // JSON-RPC requires (a notification is never answered).
            Incoming::Notification { method, .. } => {
                tracing::debug!(%method, "ignoring notification from client");
                continue;
            }
            Incoming::Invalid { id, error } => (Response::failure(id, error), After::Continue),
        };
        // A client that stopped reading can leave the queue full; the kick
        // that closes it must still get through, though the response in
        // hand still goes out if the client takes it within the grace.
        let sent = tokio::select! {
            sent = send(&conn, &response) => sent,
            // Wrapped so the `watch::Ref` is gone before the await below.
            () = async { let _ = kicked.wait_for(|kicked| *kicked).await; } => {
                let _ = tokio::time::timeout(KICK_GRACE, send(&conn, &response)).await;
                false
            }
        };
        // Tile notifications begin once the handshake is done.
        if conn.api_version.is_some() && conn.tile_task.is_none() {
            conn.tile_task = Some(AbortOnDrop(tokio::spawn(forward_tiles(
                core.sessions.tile_feed(),
                conn.outbound.clone(),
            ))));
        }
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
    // queued, including the answer to `core.shutdown`. It gets a grace to
    // do so, no more: a client that was kicked, or half-closed and stopped
    // reading, must not hold the writer forever. A `JoinHandle` that has
    // completed must not be awaited again, so it is awaited a second time
    // only after an abort.
    drop(conn);
    let mut writer = writer;
    if tokio::time::timeout(KICK_GRACE, &mut writer).await.is_err() {
        writer.abort();
        writer.await.ok();
    }
    if shutdown_after {
        core.request_shutdown();
    }
}

/// Runs a detached request in a task of its own, so a handler that panics
/// still gets its request an answer (an internal error) rather than leaving
/// the client waiting on an id forever.
async fn run_detached(
    core: Arc<Core>,
    request: ersa_protocol::core_api::rpc::Request,
    peer: super::Peer,
) -> Response {
    let id = request.id.clone();
    let method = request.method.clone();
    match tokio::spawn(async move { super::handle_detached(&core, request, &peer).await }).await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!(%method, error = %e, "a request handler failed");
            let error = super::ApiError::internal(format!("{method} failed inside Core")).0;
            Response::failure(Some(id), error)
        }
    }
}

/// Forwards `session.tile` notifications to one connection until it closes.
///
/// This task, not the session, waits for room on the connection: a client
/// busy reading a long replay is slow, not stuck. Meanwhile its feed keeps
/// only the newest tile per session, so nothing piles up and nothing is
/// lost that matters.
async fn forward_tiles(mut tiles: crate::sessions::TileFeed, outbound: mpsc::Sender<String>) {
    loop {
        for line in tiles.next().await {
            if outbound.send(line.as_ref().clone()).await.is_err() {
                return;
            }
        }
    }
}

/// A task that is aborted when this is dropped, so a connection's helper
/// tasks end with it.
pub(super) struct AbortOnDrop(pub(super) tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
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
