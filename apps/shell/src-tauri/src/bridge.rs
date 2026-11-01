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

struct Shared {
    outbound: mpsc::Sender<String>,
    pending: Pending,
    next_id: AtomicI64,
    closed: AtomicBool,
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
    /// `on_event` runs on the reader task for every notification and once
    /// when the connection closes; it must not block.
    pub async fn open(
        stream: BoxStream,
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
        });

        let writer = tokio::spawn(async move {
            while let Some(mut line) = lines.recv().await {
                line.push('\n');
                if write.write_all(line.as_bytes()).await.is_err() || write.flush().await.is_err() {
                    break;
                }
            }
        });

        let reader_shared = Arc::clone(&shared);
        let reader = tokio::spawn(async move {
            let mut read = BufReader::new(read);
            let reason = loop {
                match next_line(&mut read, MAX_LINE_BYTES).await {
                    Ok(Some(line)) => dispatch(&reader_shared, &line, &on_event),
                    Ok(None) => break "Core closed the connection".to_string(),
                    Err(e) => break format!("cannot read from Core: {e}"),
                }
            };
            close(&reader_shared);
            on_event(LinkEvent::Closed { reason });
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
        if self.shared.outbound.send(line).await.is_err() {
            self.forget(id);
            return Err(BridgeError::unavailable("the connection to Core is closed"));
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(Ok(result))) => Ok(result),
            Ok(Ok(Err(error))) => Err(BridgeError::Rpc { error }),
            // The sender was dropped: the connection closed.
            Ok(Err(_)) => Err(BridgeError::unavailable("the connection to Core closed")),
            Err(_) => {
                self.forget(id);
                Err(BridgeError::unavailable(format!(
                    "Core did not answer {method} within {REQUEST_TIMEOUT:?}"
                )))
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
        close(&self.shared);
    }
}

/// Marks the link closed and fails every request still waiting.
fn close(shared: &Shared) {
    shared.closed.store(true, Ordering::SeqCst);
    // Dropping the senders wakes each waiter with "connection closed".
    shared.pending.lock().expect("pending lock").clear();
}

/// Routes one line from Core: a response to its waiting request, a
/// notification to `on_event`. Anything else is not something Core sends and
/// is dropped.
fn dispatch(shared: &Shared, line: &str, on_event: &impl Fn(LinkEvent)) {
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
            on_event(LinkEvent::Notification { method, params });
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
