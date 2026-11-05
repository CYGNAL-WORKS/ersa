//! The connection to Core that the Shell owns on the Interface's behalf.
//!
//! The Interface never opens the pipe itself (ARCH-CORE: it touches no
//! process, file or network). It hands the Shell a Core API method and params;
//! the Shell sends them as a JSON-RPC request and hands back the result or the
//! error object unchanged, so the Interface still speaks `PROTO-CORE-API` and
//! nothing else (ARCH-CORE-R02). Notifications travel the other way the same.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ersa_core::platform::BoxStream;
use ersa_protocol::core_api::rpc::{ErrorObject, Request, RequestId, code};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

/// How long most requests may wait for Core's answer.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How long requests that run git, start a backend or probe a host may
/// wait: those can legitimately take minutes on a large repository. A
/// session's first message waits for its backend to start, so
/// `session.send` is one of them.
pub const LONG_REQUEST_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// How long the liveness probe may take to connect and complete a handshake.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// The Core API versions this Shell's Interface speaks.
pub const API_VERSIONS: &[u32] = &[3];

/// The longest line we read from Core: the same cap Core puts on the lines it reads.
const MAX_LINE_BYTES: usize = ersa_core::state::MAX_LINE_BYTES;

/// How long `method` may wait for its answer.
pub fn timeout_for(method: &str) -> Duration {
    match method {
        "workspace.create" | "workspace.remove" | "session.create" | "session.start"
        | "session.resume" | "session.stop" | "session.remove" | "session.send" | "host.probe"
        | "core.shutdown" => LONG_REQUEST_TIMEOUT,
        _ => REQUEST_TIMEOUT,
    }
}

/// Why a request got no result. Sent to the Interface as
/// `{ "kind": "rpc", "error": <JSON-RPC error object> }` or
/// `{ "kind": "unavailable", "message": "..." }`.
#[derive(Debug, Clone, PartialEq, Serialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BridgeError {
    /// Core answered with an error.
    #[error("The request failed: {}", error.message)]
    Rpc { error: ErrorObject },
    /// No answer: not attached, the connection closed, or Core timed out.
    #[error("{message}")]
    Unavailable { message: String },
}

impl BridgeError {
    pub fn unavailable(message: impl Into<String>) -> Self {
        BridgeError::Unavailable {
            message: message.into(),
        }
    }
}

/// Something Core sent that nobody asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkEvent {
    /// A JSON-RPC notification, such as `session.event`.
    Notification { method: String, params: Value },
    /// The connection is gone; every later request fails.
    Closed { reason: String },
}

/// How a link decides that Core is wedged, and its timeouts.
#[derive(Debug, Clone, Default)]
pub struct LinkOptions {
    /// When a request times out, the link opens a second connection here and
    /// completes a handshake. Core answers each connection on its own task, so
    /// a slow request on this one does not delay the probe; a Core that
    /// cannot answer the probe either is wedged, and the link closes. `None`
    /// never probes: a timeout fails only its own request.
    pub probe_endpoint: Option<String>,
    /// Replaces every method's timeout. For tests.
    pub timeout: Option<Duration>,
}

type Pending = Mutex<HashMap<i64, oneshot::Sender<Result<Value, ErrorObject>>>>;

type OnEvent = Box<dyn Fn(LinkEvent) + Send + Sync>;

struct Shared {
    outbound: mpsc::Sender<String>,
    pending: Pending,
    next_id: AtomicI64,
    closed: AtomicBool,
    on_event: OnEvent,
    options: LinkOptions,
    /// The reader and writer, stopped when the link closes so a link that
    /// was given up delivers nothing more.
    tasks: Mutex<Vec<AbortHandle>>,
}

/// One open, handshaken connection to Core. Dropping it closes the
/// connection; Core keeps running (ADR 0011).
pub struct CoreLink {
    shared: Arc<Shared>,
    hello: Value,
}

impl CoreLink {
    /// Starts reading and writing on `stream` and sends `core.hello`.
    /// `on_event` runs for every notification, and once when the connection
    /// closes for any reason but the link being dropped; it must not block.
    pub async fn open(
        stream: BoxStream,
        options: LinkOptions,
        on_event: impl Fn(LinkEvent) + Send + Sync + 'static,
    ) -> Result<CoreLink, BridgeError> {
        let (read, mut write) = tokio::io::split(stream);
        // The queue only smooths bursts; requests wait when it is full.
        let (outbound, mut lines) = mpsc::channel::<String>(64);
        let shared = Arc::new(Shared {
            outbound,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(1),
            closed: AtomicBool::new(false),
            on_event: Box::new(on_event),
            options,
            tasks: Mutex::new(Vec::new()),
        });

        let writer_shared = Arc::clone(&shared);
        let writer = tokio::spawn(async move {
            while let Some(mut line) = lines.recv().await {
                line.push('\n');
                if let Err(e) = write_line(&mut write, &line).await {
                    close(
                        &writer_shared,
                        Some(format!("Ersa can't write to the connection. {e}")),
                    );
                    break;
                }
            }
        });

        let reader_shared = Arc::clone(&shared);
        let reader = tokio::spawn(async move {
            let mut read = BufReader::new(read);
            let reason = loop {
                match next_line(&mut read, MAX_LINE_BYTES).await {
                    Ok(Some(line)) => dispatch(&reader_shared, &line),
                    Ok(None) => break "The connection closed.".to_string(),
                    Err(e) => break format!("Ersa can't read from the connection. {e}"),
                }
            };
            close(&reader_shared, Some(reason));
        });
        shared
            .tasks
            .lock()
            .expect("tasks lock")
            .extend([writer.abort_handle(), reader.abort_handle()]);

        let mut link = CoreLink {
            shared,
            hello: Value::Null,
        };
        let params = json!({
            "client": "ersa-shell",
            "client_version": env!("CARGO_PKG_VERSION"),
            "api_versions": API_VERSIONS,
        });
        link.hello = link.request("core.hello", Some(params)).await?;
        Ok(link)
    }

    /// Core's answer to `core.hello` on this connection.
    pub fn hello(&self) -> &Value {
        &self.hello
    }

    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::SeqCst)
    }

    /// Sends one request and waits for its result.
    pub async fn request(&self, method: &str, params: Option<Value>) -> Result<Value, BridgeError> {
        let id = self.shared.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.shared
            .pending
            .lock()
            .expect("pending lock")
            .insert(id, tx);
        // Checked after registering, so a close that races this request
        // either sees the entry and fails it, or is seen here.
        if self.is_closed() {
            self.forget(id);
            return Err(BridgeError::unavailable("The connection is closed."));
        }
        let mut request = Request::new(RequestId::Number(id), method, Value::Null);
        request.params = params;
        let line = serde_json::to_string(&request).expect("requests always serialize");
        // Core closes the connection on a line past its cap, which would fail
        // every other request too; refuse this one alone instead.
        if line.len() > MAX_LINE_BYTES {
            self.forget(id);
            return Err(BridgeError::unavailable(format!(
                "The request is too large to send. {method} is {} bytes, more than one line may hold.",
                line.len()
            )));
        }
        let timeout = self
            .shared
            .options
            .timeout
            .unwrap_or_else(|| timeout_for(method));
        // The timeout covers queueing the line as well as the answer: a Core
        // that stops reading fills the queue, and the send would wait forever.
        let answer = tokio::time::timeout(timeout, async {
            if self.shared.outbound.send(line).await.is_err() {
                return None;
            }
            rx.await.ok()
        })
        .await;
        match answer {
            Ok(Some(Ok(result))) => Ok(result),
            Ok(Some(Err(error))) => Err(BridgeError::Rpc { error }),
            // The writer stopped, or the waiter was dropped: the connection closed.
            Ok(None) => {
                self.forget(id);
                Err(BridgeError::unavailable("The connection closed."))
            }
            Err(_) => {
                // Only this request fails, unless the probe finds Core wedged.
                self.forget(id);
                let message = format!("Ersa got no answer in time. {method} waited {timeout:?}");
                if let Some(endpoint) = &self.shared.options.probe_endpoint
                    && !Box::pin(alive(endpoint)).await
                {
                    close(
                        &self.shared,
                        Some(format!(
                            "{message}, and a new connection got no answer either."
                        )),
                    );
                }
                Err(BridgeError::unavailable(message))
            }
        }
    }

    fn forget(&self, id: i64) {
        self.shared
            .pending
            .lock()
            .expect("pending lock")
            .remove(&id);
    }
}

impl Drop for CoreLink {
    fn drop(&mut self) {
        close(&self.shared, None);
    }
}

/// Whether the Core at `endpoint` completes a handshake on a new connection
/// within [`PROBE_TIMEOUT`].
pub async fn alive(endpoint: &str) -> bool {
    let probe = async {
        let stream = ersa_core::connect(endpoint).await.ok()?;
        CoreLink::open(stream, LinkOptions::default(), |_| {})
            .await
            .ok()
    };
    matches!(
        tokio::time::timeout(PROBE_TIMEOUT, probe).await,
        Ok(Some(_))
    )
}

/// Marks the link closed, stops its reader and writer, and fails every
/// request still waiting. The first close with a reason tells `on_event`; a
/// drop (no reason) tells nobody, since the Shell itself let go.
fn close(shared: &Shared, reason: Option<String>) {
    let was_closed = shared.closed.swap(true, Ordering::SeqCst);
    for task in shared.tasks.lock().expect("tasks lock").drain(..) {
        task.abort();
    }
    // Dropping the senders wakes each waiter with "connection closed".
    shared.pending.lock().expect("pending lock").clear();
    if let (false, Some(reason)) = (was_closed, reason) {
        (shared.on_event)(LinkEvent::Closed { reason });
    }
}

async fn write_line(
    write: &mut (impl tokio::io::AsyncWrite + Unpin),
    line: &str,
) -> std::io::Result<()> {
    write.write_all(line.as_bytes()).await?;
    write.flush().await
}

/// Routes one line from Core: a response to its waiting request, a
/// notification to `on_event`. Anything else is not something Core sends and
/// is dropped, as is everything once the link has closed.
fn dispatch(shared: &Shared, line: &str) {
    if shared.closed.load(Ordering::SeqCst) {
        return;
    }
    let Ok(Value::Object(mut message)) = serde_json::from_str::<Value>(line) else {
        return;
    };
    match (message.remove("id"), message.remove("method")) {
        (Some(id), None) => {
            let Some(id) = id.as_i64() else { return };
            let Some(waiter) = shared.pending.lock().expect("pending lock").remove(&id) else {
                return;
            };
            let outcome = match (message.remove("result"), message.remove("error")) {
                (Some(result), None) => Ok(result),
                (None, Some(error)) => Err(serde_json::from_value::<ErrorObject>(error)
                    .unwrap_or_else(|e| {
                        ErrorObject::protocol(
                            code::INTERNAL_ERROR,
                            format!("Ersa got an error it can't read. {e}"),
                        )
                    })),
                _ => Err(ErrorObject::protocol(
                    code::INTERNAL_ERROR,
                    "Ersa got an answer with neither a result nor an error.",
                )),
            };
            // The waiter may have timed out and gone; nothing to do then.
            let _ = waiter.send(outcome);
        }
        (None, Some(Value::String(method))) => {
            let params = message.remove("params").unwrap_or(Value::Null);
            (shared.on_event)(LinkEvent::Notification { method, params });
        }
        _ => {}
    }
}

/// The next line from Core without its terminator, or `None` at end of
/// input. A line longer than `max` bytes, or one that is not UTF-8, is an
/// error, so a misbehaving peer cannot make the Shell buffer without limit.
///
/// Core's own capped reader is private to it, hence this small one.
async fn next_line<R: AsyncBufRead + Unpin>(
    read: &mut R,
    max: usize,
) -> std::io::Result<Option<String>> {
    let invalid = |why: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, why.to_string());
    let mut line = Vec::new();
    // `take` stops one byte past the cap, which is enough to tell a line of
    // exactly `max` bytes from a longer one.
    let read_bytes = (&mut *read)
        .take(max as u64 + 1)
        .read_until(b'\n', &mut line)
        .await?;
    if read_bytes == 0 {
        return Ok(None);
    }
    if line.last() == Some(&b'\n') {
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
    } else if line.len() > max {
        return Err(invalid("A line is longer than the cap."));
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|_| invalid("A line is not UTF-8."))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn lines_of(input: &[u8], max: usize) -> Vec<std::io::Result<Option<String>>> {
        let mut read = BufReader::new(input);
        let mut out = Vec::new();
        loop {
            let next = next_line(&mut read, max).await;
            let done = !matches!(next, Ok(Some(_)));
            out.push(next);
            if done {
                return out;
            }
        }
    }

    #[tokio::test]
    async fn reads_lines_up_to_the_cap() {
        // The cap counts a `\r` before the newline, as Core's reader does.
        let got = lines_of(b"abc\r\nefgh\nlast", 4).await;
        let got: Vec<_> = got.into_iter().map(|r| r.unwrap()).collect();
        assert_eq!(
            got,
            vec![
                Some("abc".to_string()),
                Some("efgh".to_string()),
                Some("last".to_string()),
                None
            ]
        );
    }

    #[tokio::test]
    async fn a_line_past_the_cap_is_an_error() {
        let got = lines_of(b"abcde\n", 4).await;
        assert_eq!(got.len(), 1);
        assert!(got[0].is_err());
    }

    #[tokio::test]
    async fn a_line_that_is_not_utf8_is_an_error() {
        let got = lines_of(b"\xff\n", 4).await;
        assert!(got[0].is_err());
    }
}

/// The link against a fake Core on an in-memory pipe, for what a real Core
/// will not do on demand: answer slowly, stop answering, or send a
/// notification.
#[cfg(test)]
mod link_tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, DuplexStream};

    const SHORT: Duration = Duration::from_millis(300);

    /// An endpoint nothing serves, for a probe that must fail.
    fn nobody() -> String {
        if cfg!(windows) {
            format!(r"\\.\pipe\ersa-shell-test-nobody-{}", std::process::id())
        } else {
            std::env::temp_dir()
                .join(format!(
                    "ersa-shell-test-nobody-{}.sock",
                    std::process::id()
                ))
                .to_string_lossy()
                .into_owned()
        }
    }

    /// Opens a link to a fake Core that answers `core.hello` and nothing
    /// else, and returns the Core's end with the events the link reported.
    async fn open_fake(
        options: LinkOptions,
    ) -> (
        CoreLink,
        BufReader<DuplexStream>,
        mpsc::UnboundedReceiver<LinkEvent>,
    ) {
        let (shell_end, core_end) = tokio::io::duplex(1024);
        let (events, received) = mpsc::unbounded_channel();
        let open = CoreLink::open(Box::new(shell_end), options, move |e| {
            let _ = events.send(e);
        });
        let mut core = BufReader::new(core_end);
        let answer = async {
            let mut hello = String::new();
            core.read_line(&mut hello).await.expect("hello arrives");
            reply_to(&mut core, &hello, json!({ "api_version": 1 })).await;
        };
        let (link, ()) = tokio::join!(open, answer);
        (link.expect("handshake"), core, received)
    }

    async fn reply_to(core: &mut BufReader<DuplexStream>, request: &str, result: Value) {
        let id = serde_json::from_str::<Value>(request).expect("json")["id"].clone();
        let reply = json!({ "jsonrpc": "2.0", "id": id, "result": result });
        core.get_mut()
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .expect("reply");
    }

    fn short() -> LinkOptions {
        LinkOptions {
            probe_endpoint: None,
            timeout: Some(SHORT),
        }
    }

    #[test]
    fn long_methods_get_long_timeouts() {
        assert_eq!(timeout_for("workspace.create"), LONG_REQUEST_TIMEOUT);
        assert_eq!(timeout_for("session.create"), LONG_REQUEST_TIMEOUT);
        assert_eq!(timeout_for("session.send"), LONG_REQUEST_TIMEOUT);
        assert_eq!(timeout_for("host.probe"), LONG_REQUEST_TIMEOUT);
        assert_eq!(timeout_for("project.list"), REQUEST_TIMEOUT);
    }

    #[tokio::test]
    async fn a_slow_request_fails_alone() {
        let (link, mut core, _events) = open_fake(short()).await;
        let slow = link.request("core.status", None).await;
        assert!(
            matches!(slow, Err(BridgeError::Unavailable { .. })),
            "{slow:?}"
        );
        assert!(!link.is_closed(), "one slow request must not drop the link");

        // Core gets to it late, then answers the next request on time; the
        // late answer goes nowhere and the next one reaches its caller.
        let mut first = String::new();
        core.read_line(&mut first).await.expect("first request");
        reply_to(&mut core, &first, json!("late")).await;
        let next = async {
            let mut second = String::new();
            core.read_line(&mut second).await.expect("second request");
            reply_to(&mut core, &second, json!([])).await;
        };
        let (answer, ()) = tokio::join!(link.request("project.list", None), next);
        assert_eq!(answer, Ok(json!([])));
    }

    #[tokio::test]
    async fn a_core_that_fails_the_probe_closes_the_link() {
        let options = LinkOptions {
            probe_endpoint: Some(nobody()),
            ..short()
        };
        let (link, _core, mut events) = open_fake(options).await;
        let result = link.request("core.status", None).await;
        assert!(
            matches!(result, Err(BridgeError::Unavailable { .. })),
            "{result:?}"
        );
        assert!(link.is_closed(), "the next attach must connect anew");
        assert!(matches!(
            events.recv().await,
            Some(LinkEvent::Closed { .. })
        ));
    }

    #[tokio::test]
    async fn a_closed_link_delivers_nothing_more() {
        let options = LinkOptions {
            probe_endpoint: Some(nobody()),
            ..short()
        };
        let (link, mut core, mut events) = open_fake(options).await;
        let _ = link.request("core.status", None).await;
        assert!(matches!(
            events.recv().await,
            Some(LinkEvent::Closed { .. })
        ));
        // The old connection's Core end may still write; nothing reaches the sink.
        let notice = json!({ "jsonrpc": "2.0", "method": "core.notice", "params": {} });
        let _ = core
            .get_mut()
            .write_all(format!("{notice}\n").as_bytes())
            .await;
        tokio::time::sleep(SHORT).await;
        assert!(events.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_request_past_the_line_cap_is_refused_alone() {
        let (link, _core, _events) = open_fake(short()).await;
        let huge = "x".repeat(MAX_LINE_BYTES);
        let result = link
            .request("settings.set", Some(json!({ "patch": { "k": huge } })))
            .await;
        assert!(
            matches!(result, Err(BridgeError::Unavailable { .. })),
            "{result:?}"
        );
        assert!(!link.is_closed());
    }

    #[tokio::test]
    async fn notifications_reach_the_event_callback() {
        let (_link, mut core, mut events) = open_fake(short()).await;
        let notice = json!({ "jsonrpc": "2.0", "method": "core.notice", "params": { "level": "info", "message": "hi" } });
        core.get_mut()
            .write_all(format!("{notice}\n").as_bytes())
            .await
            .expect("notice");
        assert_eq!(
            events.recv().await,
            Some(LinkEvent::Notification {
                method: "core.notice".into(),
                params: json!({ "level": "info", "message": "hi" }),
            })
        );
    }
}
