//! Reports how many messages a live session's CLI holds behind its running
//! turn (PROTO-CLAUDE-CODE-STREAM-R46), for the tile's `waiting`
//! (PROTO-CORE-API-R81).
//!
//! The count comes from the CLI's own record ([`WaitingCount`]). It is read
//! when it can have changed: when the adapter hands the CLI a message while
//! a turn runs (the message waits), when a turn starts (the CLI has taken
//! every waiting message into it), and, while messages wait, at the running
//! turn's next model call and at its end (a turn that uses tools takes them
//! in at its next model call, with no new turn). Each read
//! runs on a blocking thread, one at a time per session, and the sink hears
//! the count only when it differs from the last one it heard.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ersa_adapter::AdapterSink;
use time::OffsetDateTime;

use crate::record::{self, WaitingCount};

/// How long a read after a handover waits for the CLI to write the message's
/// `enqueue` entry. The CLI writes it as soon as it reads the message (1 ms
/// after the write in fixtures 22 and 24), so this allows for a busy disk.
const ENQUEUE_WAIT: Duration = Duration::from_secs(2);

/// How often a read after a handover looks again for the `enqueue` entry.
const ENQUEUE_POLL: Duration = Duration::from_millis(50);

/// One CLI process's count of waiting messages, and what the sink last heard.
pub(crate) struct WaitingReader {
    home: PathBuf,
    cwd: String,
    session_id: String,
    count: Mutex<WaitingCount>,
    /// User messages written to this process. The CLI writes one `enqueue`
    /// entry for each, so a read after a handover knows the message's entry
    /// is in once the record holds this many.
    written: AtomicU32,
    /// Whether the CLI is running a turn, as far as its stdout and the
    /// messages written to it say: from a user message or a `system/init`
    /// until the turn's `result`.
    turn_running: AtomicBool,
    /// The count the sink last heard; it starts at none waiting.
    reported: AtomicU32,
    /// Held across a read, so reads run one at a time.
    reading: tokio::sync::Mutex<()>,
}

impl WaitingReader {
    /// A reader for session `session_id`'s record, under `cwd` in the CLI's
    /// home `home`, counting the entries of a process started at `started`.
    pub(crate) fn new(
        home: PathBuf,
        cwd: String,
        session_id: String,
        started: OffsetDateTime,
    ) -> Self {
        WaitingReader {
            home,
            cwd,
            session_id,
            count: Mutex::new(WaitingCount::new(started)),
            written: AtomicU32::new(0),
            turn_running: AtomicBool::new(false),
            reported: AtomicU32::new(0),
            reading: tokio::sync::Mutex::new(()),
        }
    }

    /// Whether a message handed over now will wait in the CLI: a turn is
    /// running, or messages already wait for the next one.
    pub(crate) fn busy(&self) -> bool {
        self.turn_running.load(Ordering::SeqCst) || self.holds_some()
    }

    /// Whether the sink was last told some messages wait.
    pub(crate) fn holds_some(&self) -> bool {
        self.reported.load(Ordering::SeqCst) > 0
    }

    /// A user message was written to the CLI. It starts a turn if none was
    /// running.
    pub(crate) fn wrote_message(&self) {
        self.written.fetch_add(1, Ordering::SeqCst);
        self.turn_running.store(true, Ordering::SeqCst);
    }

    /// The CLI started a turn (`system/init`).
    pub(crate) fn turn_started(&self) {
        self.turn_running.store(true, Ordering::SeqCst);
    }

    /// The CLI ended a turn (`result`).
    pub(crate) fn turn_ended(&self) {
        self.turn_running.store(false, Ordering::SeqCst);
    }

    /// Reads the record and tells `sink` the count if it changed. After a
    /// handover (`after_handover`), it first waits, for up to
    /// [`ENQUEUE_WAIT`], for the CLI to write the message's `enqueue`, and
    /// then reports what it read either way. Nothing is reported once
    /// `alive` says the session is going away. A record that cannot be read
    /// is logged and reports nothing.
    pub(crate) async fn report(
        self: Arc<Self>,
        sink: Arc<dyn AdapterSink>,
        after_handover: bool,
        alive: impl Fn() -> bool + Send,
    ) {
        let _one_at_a_time = self.reading.lock().await;
        let deadline = tokio::time::Instant::now() + ENQUEUE_WAIT;
        let waiting = loop {
            let reader = Arc::clone(&self);
            let read = tokio::task::spawn_blocking(move || reader.read()).await;
            let (waiting, enqueued) = match read {
                Ok(Ok(read)) => read,
                Ok(Err(e)) => {
                    tracing::warn!(
                        session = %self.session_id,
                        error = %e,
                        "could not read the session record for the messages Claude Code holds"
                    );
                    return;
                }
                // The blocking thread panicked or was canceled at shutdown.
                Err(_) => return,
            };
            let entry_in = enqueued >= self.written.load(Ordering::SeqCst);
            if !after_handover || entry_in || tokio::time::Instant::now() >= deadline {
                break waiting;
            }
            tokio::time::sleep(ENQUEUE_POLL).await;
        };
        if self.reported.swap(waiting, Ordering::SeqCst) != waiting && alive() {
            sink.waiting(waiting);
        }
    }

    /// The messages waiting and the messages taken, from what the record
    /// has gained since the last read. A record not written yet (a new
    /// session before its first message) holds none.
    fn read(&self) -> std::io::Result<(u32, u32)> {
        let path = record::find_record(&self.home, &self.cwd, &self.session_id)?;
        let mut count = self.count.lock().unwrap_or_else(|e| e.into_inner());
        let waiting = match path {
            Some(path) => count.read(&path)?,
            None => count.waiting(),
        };
        Ok((waiting, count.enqueued()))
    }
}
