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
use tokio::task::JoinHandle;

/// How long a request may wait for Core's answer. Core answers everything the
/// Interface sends today well within this; a Core that does not is wedged,
/// and the Interface should hear so rather than wait forever.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The Core API versions this Shell's Interface speaks.
pub const API_VERSIONS: &[u32] = &[1];

/// The longest line we read from Core: the same cap Core puts on the lines it reads.
const MAX_LINE_BYTES: usize = ersa_core::state::MAX_LINE_BYTES;

/// Why a request got no result. Sent to the Interface as
/// `{ "kind": "rpc", "error": <JSON-RPC error object> }` or
/// `{ "kind": "unavailable", "message": "..." }`.
#[derive(Debug, Clone, PartialEq, Serialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BridgeError {
    /// Core answered with an error.
    #[error("Core answered with an error: {}", error.message)]
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

type Pending = Mutex<HashMap<i64, oneshot::Sender<Result<Value, ErrorObject>>>>;

type OnEvent = Box<dyn Fn(LinkEvent) + Send + Sync>;

struct Shared {
    outbound: mpsc::Sender<String>,
    pending: Pending,
    next_id: AtomicI64,
    closed: AtomicBool,
    on_event: OnEvent,
    request_timeout: Duration,
}

/// One open, handshaken connection to Core. Dropping it closes the
/// connection; Core keeps running (ADR 0011).
pub struct CoreLink {
    shared: Arc<Shared>,
    hello: Value,
    tasks: [JoinHandle<()>; 2],
}

impl CoreLink {
    /// Starts reading and writing on `stream` and sends `core.hello`.
    /// `on_event` runs for every notification, and once when the connection
    /// closes for any reason but the link being dropped; it must not block.
    pub async fn open(
        stream: BoxStream,
        on_event: impl Fn(LinkEvent) + Send + Sync + 'static,
    ) -> Result<CoreLink, BridgeError> {
        Self::open_with_timeout(stream, REQUEST_TIMEOUT, on_event).await
    }

    /// [`CoreLink::open`] with another request timeout, so tests need not
    /// wait the real one out.
    pub async fn open_with_timeout(
        stream: BoxStream,
        request_timeout: Duration,
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
            request_timeout,
        });

        let writer_shared = Arc::clone(&shared);
        let writer = tokio::spawn(async move {
            while let Some(mut line) = lines.recv().await {
                line.push('\n');
                if let Err(e) = write_line(&mut write, &line).await {
                    close(&writer_shared, Some(format!("cannot write to Core: {e}")));
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
                    Ok(None) => break "Core closed the connection".to_string(),
                    Err(e) => break format!("cannot read from Core: {e}"),
                }
            };
            close(&reader_shared, Some(reason));
        });

        let mut link = CoreLink {
            shared,
            hello: Value::Null,
            tasks: [writer, reader],
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
            return Err(BridgeError::unavailable("the connection to Core is closed"));
        }
        let mut request = Request::new(RequestId::Number(id), method, Value::Null);
        request.params = params;
        let line = serde_json::to_string(&request).expect("requests always serialize");
        // Core closes the connection on a line past its cap, which would fail
        // every other request too; refuse this one alone instead.
        if line.len() > MAX_LINE_BYTES {
            self.forget(id);
            return Err(BridgeError::unavailable(format!(
                "{method} is {} bytes, more than Core accepts on one line",
                line.len()
            )));
        }
        // The timeout covers queueing the line as well as the answer: a Core
        // that stops reading fills the queue, and the send would wait forever.
        let answer = tokio::time::timeout(self.shared.request_timeout, async {
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
                Err(BridgeError::unavailable("the connection to Core closed"))
            }
            Err(_) => {
                // Core is wedged: it holds the connection open and answers
                // nothing. Give the link up so the next attach connects anew.
                let message = format!(
                    "Core did not answer {method} within {:?}",
                    self.shared.request_timeout
                );
                close(&self.shared, Some(message.clone()));
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
        for task in &self.tasks {
            task.abort();
        }
        close(&self.shared, None);
    }
}

/// Marks the link closed and fails every request still waiting. The first
/// close with a reason tells `on_event`; a drop (no reason) tells nobody,
/// since the Shell itself let go.
fn close(shared: &Shared, reason: Option<String>) {
    let was_closed = shared.closed.swap(true, Ordering::SeqCst);
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
/// is dropped.
fn dispatch(shared: &Shared, line: &str) {
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
                            format!("Core sent an error the Shell cannot read: {e}"),
                        )
                    })),
                _ => Err(ErrorObject::protocol(
                    code::INTERNAL_ERROR,
                    "Core sent a response with neither a result nor an error",
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
        return Err(invalid("a line from Core is longer than the cap"));
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|_| invalid("a line from Core is not UTF-8"))
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
/// will not do on demand: stop answering, or send a notification.
#[cfg(test)]
mod link_tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, DuplexStream};

    const SHORT: Duration = Duration::from_millis(300);

    /// Opens a link to a fake Core that answers `core.hello` and nothing
    /// else, and returns the Core's end with the events the link reported.
    async fn open_fake() -> (
        CoreLink,
        BufReader<DuplexStream>,
        mpsc::UnboundedReceiver<LinkEvent>,
    ) {
        let (shell_end, core_end) = tokio::io::duplex(1024);
        let (events, received) = mpsc::unbounded_channel();
        let open = CoreLink::open_with_timeout(Box::new(shell_end), SHORT, move |e| {
            let _ = events.send(e);
        });
        let mut core = BufReader::new(core_end);
        let answer = async {
            let mut hello = String::new();
            core.read_line(&mut hello).await.expect("hello arrives");
            let id = serde_json::from_str::<Value>(&hello).expect("json")["id"].clone();
            let reply = json!({ "jsonrpc": "2.0", "id": id, "result": { "api_version": 1 } });
            core.get_mut()
                .write_all(format!("{reply}\n").as_bytes())
                .await
                .expect("reply");
        };
        let (link, ()) = tokio::join!(open, answer);
        (link.expect("handshake"), core, received)
    }

    #[tokio::test]
    async fn a_core_that_stops_answering_closes_the_link() {
        let (link, _core, mut events) = open_fake().await;
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
    async fn a_request_past_the_line_cap_is_refused_alone() {
        let (link, _core, _events) = open_fake().await;
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
        let (_link, mut core, mut events) = open_fake().await;
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
