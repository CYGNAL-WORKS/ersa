//! A fake backend adapter, for testing the session manager without a CLI.
//!
//! Its "backend" lives in the adapter itself: each user message is answered
//! at once with backend lines recorded to the sink, so a test controls
//! exactly what the raw log holds. Like Claude Code, the backend also keeps
//! a session record ([`Records`]), which outlives any Core and which a test
//! can add to, as turns taken elsewhere would. Messages that start with a keyword ask
//! for something special:
//!
//! - `ask`: a permission request, answered with `session.decide_permission`.
//! - `question`: a question for the user.
//! - `burst <n>`: `n` replies at once, to flood subscribers.
//! - `work`: a turn that keeps working, the backend saying it is running,
//!   until it is interrupted, then ends with outcome `interrupted`.
//! - `idle`: the backend says it is idle, as Claude Code does once a turn
//!   ends; the fake otherwise reports itself running from its start.
//! - `broken pipe`: the write fails halfway, so Core can't tell whether the
//!   backend has the message.
//! - `stopped mid-write`: the line is recorded, then the session is found
//!   stopped, as a kill landing during the write would leave it.
//!
//! Every message is kept in [`Stats::messages`] as the fake was given it,
//! attachments and all; the fake takes PNG and GIF images of up to
//! [`IMAGE_MAX`] bytes.
//!
//! A rename keeps the new title in the record too, as a `custom-title`
//! entry, which titles the session ahead of its last prompt, as Claude
//! Code's does.
//!
//! Anything else gets a usage line and an echoed reply.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, BackendChoices, Direction, ImageLimits,
    OpenElsewhere, OutgoingMessage, PermissionDecision, ProcessExit, Projected, Projector, RawLine,
    RecordDeletion, RecordEvent, RecordSummary, SessionConfig, Transport, async_trait,
};
use ersa_protocol::core_api::Choice;
use ersa_protocol::events::{
    AssistantText, BackendPassthrough, DecidedBy, Event, LifecycleState, MessageOrigin,
    PermissionBehavior, PermissionDecided, PermissionFlags, PermissionRequested, Question,
    QuestionAsked, SessionStarted, SessionState, TurnCompleted, TurnOutcome, TurnStarted,
    TurnTrigger, UsageUpdated, UserMessage,
};
use serde_json::{Map, Value, json};

pub const BACKEND: &str = "fake";

/// The largest image the fake takes.
pub const IMAGE_MAX: u64 = 1024;

/// What the fake has been asked to do, shared with the test.
#[derive(Debug, Default)]
pub struct Stats {
    pub starts: AtomicUsize,
    pub stops: AtomicUsize,
    /// The configuration of every session started, in order.
    pub configs: Mutex<Vec<SessionConfig>>,
    /// Renames sent to a running backend, and renames written to a record.
    pub live_renames: AtomicUsize,
    pub record_renames: AtomicUsize,
    /// For every time Core asked whether another process has a session
    /// open: the backend's id for it, and how many record renames had
    /// been written by then.
    pub open_checks: Mutex<Vec<(String, usize)>>,
    /// Every message the fake was given, in order.
    pub messages: Mutex<Vec<OutgoingMessage>>,
    /// Records deleted.
    pub record_deletes: AtomicUsize,
}

/// The kind of the record entry a rename adds.
const CUSTOM_TITLE: &str = "custom-title";

/// A title the user gave the session, as the fake's record keeps it.
fn recorded_title(title: &str) -> RecordEvent {
    RecordEvent {
        message_id: None,
        ts: None,
        backend_version: Some("1.0".into()),
        event: Event::BackendPassthrough(BackendPassthrough {
            backend: BACKEND.into(),
            kind: CUSTOM_TITLE.into(),
            payload: json!({ "title": title }),
        }),
    }
}

/// The title a record entry gives, when it is a rename.
fn title_of(event: &RecordEvent) -> Option<String> {
    match &event.event {
        Event::BackendPassthrough(p) if p.kind == CUSTOM_TITLE => {
            p.payload["title"].as_str().map(String::from)
        }
        _ => None,
    }
}

/// The fake backend's session records, by its own session id: what it
/// keeps of each session, as Claude Code keeps its session files. Share one
/// between the Cores of a test to have the record outlive a Core.
pub type Records = Arc<Mutex<HashMap<String, Vec<RecordEvent>>>>;

/// Numbers the fake backend's sessions across every Core in the test run.
static NEXT_SESSION: AtomicUsize = AtomicUsize::new(1);

/// A user message as the fake's record keeps it.
pub fn recorded_user(id: &str, text: &str) -> RecordEvent {
    RecordEvent {
        message_id: Some(id.into()),
        ts: Some("2026-11-01T00:00:00.000Z".into()),
        backend_version: Some("1.0".into()),
        event: Event::UserMessage(UserMessage {
            text: text.into(),
            origin: MessageOrigin::Human,
            peer: None,
            attachments: Vec::new(),
        }),
    }
}

/// A reply as the fake's record keeps it.
pub fn recorded_reply(id: &str, text: &str) -> RecordEvent {
    RecordEvent {
        message_id: Some(id.into()),
        ts: Some("2026-11-01T00:00:01.000Z".into()),
        backend_version: Some("1.0".into()),
        event: Event::AssistantText(AssistantText {
            block_id: format!("{id}/0"),
            text: text.into(),
            is_final: true,
        }),
    }
}

pub struct FakeAdapter {
    projector_version: u32,
    fail_start: bool,
    panic_start: bool,
    /// How long the handshake takes.
    start_delay: std::time::Duration,
    pub stats: Arc<Stats>,
    pub records: Records,
    /// What the fake's registry of open sessions answers.
    pub open_elsewhere: Arc<Mutex<OpenElsewhere>>,
    /// Held by a test to hold every registry check until it lets go.
    pub open_check_gate: Arc<tokio::sync::Mutex<()>>,
    /// When set, what deleting a record answers instead of deleting it: the
    /// registry saying so at the moment of deletion.
    pub delete_refusal: Arc<Mutex<Option<OpenElsewhere>>>,
    /// A backend that takes a stop and never exits.
    stop_hangs: bool,
}

impl FakeAdapter {
    pub fn new(projector_version: u32) -> Self {
        FakeAdapter {
            projector_version,
            fail_start: false,
            panic_start: false,
            start_delay: std::time::Duration::ZERO,
            stats: Arc::default(),
            records: Arc::default(),
            open_elsewhere: Arc::new(Mutex::new(OpenElsewhere::No)),
            open_check_gate: Arc::default(),
            delete_refusal: Arc::default(),
            stop_hangs: false,
        }
    }

    /// An adapter whose backend never exits when stopped.
    pub fn unstoppable() -> Self {
        FakeAdapter {
            stop_hangs: true,
            ..FakeAdapter::new(1)
        }
    }

    /// An adapter whose backend keeps its records in `records`.
    pub fn with_records(records: Records) -> Self {
        FakeAdapter {
            records,
            ..FakeAdapter::new(1)
        }
    }

    /// An adapter whose backend takes `delay` to become ready.
    pub fn slow_start(delay: std::time::Duration) -> Self {
        FakeAdapter {
            start_delay: delay,
            ..FakeAdapter::new(1)
        }
    }

    /// An adapter with a bug: it panics while starting.
    pub fn panicking() -> Self {
        FakeAdapter {
            panic_start: true,
            ..FakeAdapter::new(1)
        }
    }

    /// An adapter whose backend exits with code 1 during the handshake.
    pub fn failing() -> Self {
        FakeAdapter {
            fail_start: true,
            ..FakeAdapter::new(1)
        }
    }
}

#[async_trait]
impl Adapter for FakeAdapter {
    fn backend(&self) -> &str {
        BACKEND
    }

    /// Two of each, with a default marked in each list as Claude Code's are.
    fn choices(&self) -> BackendChoices {
        let list = |ids: [&str; 2], default: &str| -> Vec<Choice> {
            ids.iter()
                .map(|&id| Choice {
                    id: id.into(),
                    label: id.to_uppercase(),
                    default: id == default,
                })
                .collect()
        };
        BackendChoices {
            models: list(["model-a", "model-b"], "model-a"),
            efforts: list(["low", "high"], "high"),
            permission_modes: list(["ask", "auto"], "ask"),
        }
    }

    async fn start(
        &self,
        _transport: Arc<dyn Transport>,
        config: SessionConfig,
        sink: Arc<dyn AdapterSink>,
    ) -> Result<Box<dyn AdapterSession>, AdapterError> {
        self.stats.starts.fetch_add(1, Ordering::SeqCst);
        self.stats.configs.lock().unwrap().push(config.clone());
        sink.record(
            Direction::Host,
            json!({ "t": "hello", "cwd": config.cwd }).to_string(),
        );
        if self.panic_start {
            panic!("the fake adapter has a bug");
        }
        tokio::time::sleep(self.start_delay).await;
        if self.fail_start {
            sink.process_exited(Ok(ProcessExit { code: Some(1) }));
            return Err(AdapterError::Protocol("the fake refused to start".into()));
        }
        // Announces its own id for the session, as Claude Code's `init` does.
        let session_id = format!("fake-{}", NEXT_SESSION.fetch_add(1, Ordering::SeqCst));
        self.records
            .lock()
            .unwrap()
            .insert(session_id.clone(), Vec::new());
        sink.record(
            Direction::Backend,
            json!({ "t": "ready", "session_id": session_id }).to_string(),
        );
        Ok(Box::new(FakeSession {
            sink,
            stats: Arc::clone(&self.stats),
            records: Arc::clone(&self.records),
            session_id,
            stop_hangs: self.stop_hangs,
            inner: Mutex::default(),
        }))
    }

    fn image_limits(&self) -> ImageLimits {
        ImageLimits {
            media_types: &["image/png", "image/gif"],
            max_bytes: IMAGE_MAX,
        }
    }

    fn projector(&self) -> Box<dyn Projector> {
        Box::new(FakeProjector {
            version: self.projector_version,
            turn: 0,
        })
    }

    async fn read_record(
        &self,
        _transport: Arc<dyn Transport>,
        _cwd: &str,
        backend_session_id: &str,
    ) -> Result<Arc<Vec<RecordEvent>>, AdapterError> {
        // A title is not part of the conversation, as Claude Code's
        // projection leaves its `custom-title` entries out.
        Ok(Arc::new(
            self.records
                .lock()
                .unwrap()
                .get(backend_session_id)
                .map(|record| {
                    record
                        .iter()
                        .filter(|e| title_of(e).is_none())
                        .cloned()
                        .collect()
                })
                .unwrap_or_default(),
        ))
    }

    async fn rename_record(
        &self,
        _transport: Arc<dyn Transport>,
        _cwd: &str,
        backend_session_id: &str,
        title: &str,
    ) -> Result<(), AdapterError> {
        let mut records = self.records.lock().unwrap();
        let Some(record) = records.get_mut(backend_session_id) else {
            return Err(AdapterError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no record",
            )));
        };
        record.push(recorded_title(title));
        self.stats.record_renames.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn open_elsewhere(
        &self,
        _transport: Arc<dyn Transport>,
        backend_session_id: &str,
    ) -> OpenElsewhere {
        drop(self.open_check_gate.lock().await);
        let renames = self.stats.record_renames.load(Ordering::SeqCst);
        self.stats
            .open_checks
            .lock()
            .unwrap()
            .push((backend_session_id.to_string(), renames));
        *self.open_elsewhere.lock().unwrap()
    }

    /// Deletes the record, as Claude Code's adapter deletes its file; one
    /// already gone counts as deleted.
    async fn delete_record(
        &self,
        _transport: Arc<dyn Transport>,
        _cwd: &str,
        backend_session_id: &str,
    ) -> Result<RecordDeletion, AdapterError> {
        if let Some(refused) = *self.delete_refusal.lock().unwrap() {
            return Ok(RecordDeletion::Refused(refused));
        }
        self.records.lock().unwrap().remove(backend_session_id);
        self.stats.record_deletes.fetch_add(1, Ordering::SeqCst);
        Ok(RecordDeletion::Deleted)
    }

    /// Titled by its latest rename, else its last prompt, as Claude Code
    /// titles a print-mode session, and begun at its first entry.
    async fn summarize_record(
        &self,
        _transport: Arc<dyn Transport>,
        _cwd: &str,
        backend_session_id: &str,
    ) -> Result<Option<RecordSummary>, AdapterError> {
        let records = self.records.lock().unwrap();
        let Some(record) = records.get(backend_session_id) else {
            return Ok(None);
        };
        let title = record.iter().rev().find_map(title_of).or_else(|| {
            record.iter().rev().find_map(|e| match &e.event {
                Event::UserMessage(user) => Some(user.text.clone()),
                _ => None,
            })
        });
        Ok(Some(RecordSummary {
            title,
            created_at: record.iter().find_map(|e| e.ts.clone()),
        }))
    }
}

struct FakeSession {
    sink: Arc<dyn AdapterSink>,
    stats: Arc<Stats>,
    records: Records,
    session_id: String,
    /// Takes a stop and never exits.
    stop_hangs: bool,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    next_message: u32,
    next_request: u32,
    pending: HashSet<String>,
    /// A `work` turn is running, and an interrupt ends it.
    working: bool,
    exited: bool,
}

impl FakeSession {
    fn backend(&self, line: Value) {
        self.sink.record(Direction::Backend, line.to_string());
    }

    /// Adds to this session's record, as the backend does as it goes.
    fn keep(&self, event: RecordEvent) {
        let mut records = self.records.lock().unwrap();
        records
            .entry(self.session_id.clone())
            .or_default()
            .push(event);
    }

    /// The next message id, unique within the session.
    fn next_id(&self, prefix: &str) -> String {
        let mut inner = self.inner.lock().unwrap();
        inner.next_message += 1;
        format!("{prefix}-{}", inner.next_message)
    }

    fn exit(&self, code: Option<i32>) {
        let mut inner = self.inner.lock().unwrap();
        if !inner.exited {
            inner.exited = true;
            self.sink.process_exited(Ok(ProcessExit { code }));
        }
    }

    fn running(&self) -> Result<(), AdapterError> {
        if self.inner.lock().unwrap().exited {
            Err(AdapterError::NotRunning)
        } else {
            Ok(())
        }
    }
}

impl Drop for FakeSession {
    fn drop(&mut self) {
        self.exit(None);
    }
}

#[async_trait]
impl AdapterSession for FakeSession {
    async fn send_message(&self, message: &OutgoingMessage) -> Result<(), AdapterError> {
        self.running()?;
        if message.text == "broken pipe" {
            return Err(AdapterError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "the backend stopped reading",
            )));
        }
        if message.text == "stopped mid-write" {
            self.sink.record(
                Direction::Host,
                json!({ "t": "user", "text": message.text }).to_string(),
            );
            return Err(AdapterError::NotRunning);
        }
        self.stats.messages.lock().unwrap().push(message.clone());
        self.send_user(&message.text).await
    }

    async fn send_user(&self, text: &str) -> Result<(), AdapterError> {
        self.running()?;
        self.sink.record(
            Direction::Host,
            json!({ "t": "user", "text": text }).to_string(),
        );
        if let Some(count) = text.strip_prefix("burst ") {
            let count: usize = count.parse().expect("burst <n>");
            for i in 0..count {
                let id = self.next_id("r");
                self.backend(json!({ "t": "reply", "id": id, "text": format!("{i}") }));
            }
        } else if text == "work" {
            // The turn starts and keeps working: nothing more until an interrupt.
            self.inner.lock().unwrap().working = true;
            self.backend(json!({ "t": "working" }));
        } else if text == "finish" {
            // A turn that replies and ends, successfully.
            let id = self.next_id("r");
            self.backend(json!({ "t": "reply", "id": id, "text": "done" }));
            self.backend(json!({ "t": "completed" }));
        } else if text == "idle" {
            self.backend(json!({ "t": "idle" }));
        } else if text == "question" {
            self.backend(json!({ "t": "question", "id": "q-1" }));
        } else if text == "ask" {
            let id = {
                let mut inner = self.inner.lock().unwrap();
                inner.next_request += 1;
                let id = format!("r-{}", inner.next_request);
                inner.pending.insert(id.clone());
                id
            };
            self.backend(json!({ "t": "ask", "id": id }));
        } else {
            self.keep(recorded_user(&self.next_id("u"), text));
            self.backend(json!({ "t": "usage", "tokens": text.len() * 10 }));
            let id = self.next_id("r");
            let reply = format!("echo: {text}");
            self.backend(json!({ "t": "reply", "id": id, "text": reply }));
            self.keep(recorded_reply(&id, &reply));
        }
        Ok(())
    }

    async fn decide_permission(
        &self,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), AdapterError> {
        self.running()?;
        if !self.inner.lock().unwrap().pending.remove(request_id) {
            return Err(AdapterError::UnknownRequest(request_id.to_string()));
        }
        let behavior = match decision.behavior {
            PermissionBehavior::Allow => "allow",
            PermissionBehavior::Deny => "deny",
        };
        self.sink.record(
            Direction::Host,
            json!({ "t": "decide", "id": request_id, "behavior": behavior }).to_string(),
        );
        self.backend(json!({ "t": "done", "id": request_id }));
        Ok(())
    }

    async fn interrupt(&self) -> Result<(), AdapterError> {
        self.running()?;
        self.sink
            .record(Direction::Host, json!({ "t": "interrupt" }).to_string());
        // A working turn ends as interrupted; with none, the interrupt has no effect.
        let working = std::mem::take(&mut self.inner.lock().unwrap().working);
        if working {
            self.backend(json!({ "t": "interrupted" }));
        }
        Ok(())
    }

    /// Takes the title and keeps it in the record, as Claude Code does.
    async fn rename(&self, backend_session_id: &str, title: &str) -> Result<(), AdapterError> {
        self.running()?;
        assert_eq!(
            backend_session_id, self.session_id,
            "renamed the wrong session"
        );
        self.sink.record(
            Direction::Host,
            json!({ "t": "rename", "title": title }).to_string(),
        );
        self.keep(recorded_title(title));
        self.stats.live_renames.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn stop(&self) -> Result<(), AdapterError> {
        self.stats.stops.fetch_add(1, Ordering::SeqCst);
        if !self.stop_hangs {
            self.exit(Some(0));
        }
        Ok(())
    }
}

/// Projects the fake's lines. Pure: the same log always gives the same
/// events, whatever `version` says.
struct FakeProjector {
    version: u32,
    turn: u32,
}

impl Projector for FakeProjector {
    fn version(&self) -> u32 {
        self.version
    }

    fn project(&mut self, line: &RawLine) -> Vec<Projected> {
        let value: Value = serde_json::from_str(&line.line).unwrap_or(Value::Null);
        let text = |key: &str| value[key].as_str().unwrap_or_default().to_string();
        let events = match (line.dir, value["t"].as_str()) {
            (Direction::Host, Some("user")) => {
                self.turn += 1;
                vec![
                    Event::TurnStarted(TurnStarted {
                        turn: self.turn,
                        trigger: TurnTrigger::User,
                    }),
                    Event::UserMessage(UserMessage {
                        text: text("text"),
                        origin: MessageOrigin::Human,
                        peer: None,
                        attachments: Vec::new(),
                    }),
                ]
            }
            (Direction::Backend, Some("ready")) => vec![
                Event::SessionStarted(SessionStarted {
                    backend: BACKEND.into(),
                    backend_version: "1.0".into(),
                    model: "fake".into(),
                    cwd: String::new(),
                    capabilities: Vec::new(),
                    backend_session_id: Some(text("session_id")),
                }),
                Event::SessionState(SessionState {
                    state: LifecycleState::Running,
                    reason: None,
                }),
            ],
            (Direction::Backend, Some("idle")) => vec![Event::SessionState(SessionState {
                state: LifecycleState::Idle,
                reason: None,
            })],
            (Direction::Backend, Some("working")) => vec![Event::SessionState(SessionState {
                state: LifecycleState::Running,
                reason: None,
            })],
            (Direction::Backend, Some(ended @ ("interrupted" | "completed"))) => {
                let interrupted = ended == "interrupted";
                vec![Event::TurnCompleted(TurnCompleted {
                    turn: self.turn,
                    outcome: if interrupted {
                        TurnOutcome::Interrupted
                    } else {
                        TurnOutcome::Success
                    },
                    duration_ms: 0,
                    is_error: interrupted,
                    final_text: None,
                })]
            }
            (Direction::Backend, Some("usage")) => {
                let tokens = value["tokens"].as_u64().unwrap_or(0);
                vec![Event::UsageUpdated(UsageUpdated {
                    context_tokens: Some(tokens),
                    context_window: Some(200_000),
                    cost_usd_total: Some(tokens as f64 / 1000.0),
                    ..UsageUpdated::default()
                })]
            }
            // The record keeps replies under the same id, so Core shows a
            // reply in both once.
            (Direction::Backend, Some("reply")) => {
                let id = text("id");
                return vec![Projected::of_message(
                    Some(id.clone()),
                    Event::AssistantText(AssistantText {
                        block_id: format!("{id}/0"),
                        text: text("text"),
                        is_final: true,
                    }),
                )];
            }
            (Direction::Backend, Some("ask")) => {
                vec![Event::PermissionRequested(PermissionRequested {
                    request_id: text("id"),
                    tool_use_id: format!("tu-{}", line.index),
                    tool_name: "Bash".into(),
                    display_name: None,
                    description: None,
                    input: Map::new(),
                    blocked_path: None,
                    suggestions: Vec::new(),
                    flags: PermissionFlags::default(),
                    reason: None,
                })]
            }
            (Direction::Backend, Some("question")) => vec![Event::QuestionAsked(QuestionAsked {
                request_id: text("id"),
                questions: vec![Question {
                    id: "q".into(),
                    text: "Which one?".into(),
                    options: None,
                    multi_select: false,
                }],
            })],
            (Direction::Host, Some("decide")) => {
                vec![Event::PermissionDecided(PermissionDecided {
                    request_id: text("id"),
                    behavior: if value["behavior"] == "allow" {
                        PermissionBehavior::Allow
                    } else {
                        PermissionBehavior::Deny
                    },
                    decided_by: DecidedBy::User,
                    message: None,
                    applied_suggestions: None,
                })]
            }
            (_, kind) => vec![Event::BackendPassthrough(BackendPassthrough {
                backend: BACKEND.into(),
                kind: format!("fake/{}", kind.unwrap_or("unknown")),
                payload: value.clone(),
            })],
        };
        events
            .into_iter()
            .map(|event| Projected::from_line(line, event))
            .collect()
    }
}
