//! A fake backend adapter, for testing the session manager without a CLI.
//!
//! Its "backend" lives in the adapter itself: each user message is answered
//! at once with backend lines recorded to the sink, so a test controls
//! exactly what the raw log holds. Messages that start with a keyword ask
//! for something special:
//!
//! - `ask`: a permission request, answered with `session.decide_permission`.
//! - `burst <n>`: `n` replies at once, to flood subscribers.
//!
//! Anything else gets a usage line and an echoed reply.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, Direction, PermissionDecision, ProcessExit,
    Projected, Projector, RawLine, SessionConfig, Transport, async_trait,
};
use ersa_protocol::events::{
    AssistantText, BackendPassthrough, DecidedBy, Event, LifecycleState, MessageOrigin,
    PermissionBehavior, PermissionDecided, PermissionFlags, PermissionRequested, SessionState,
    TurnStarted, TurnTrigger, UsageUpdated, UserMessage,
};
use serde_json::{Map, Value, json};

pub const BACKEND: &str = "fake";

/// What the fake has been asked to do, shared with the test.
#[derive(Debug, Default)]
pub struct Stats {
    pub starts: AtomicUsize,
    pub stops: AtomicUsize,
}

pub struct FakeAdapter {
    projector_version: u32,
    fail_start: bool,
    pub stats: Arc<Stats>,
}

impl FakeAdapter {
    pub fn new(projector_version: u32) -> Self {
        FakeAdapter {
            projector_version,
            fail_start: false,
            stats: Arc::default(),
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

    async fn start(
        &self,
        _transport: Arc<dyn Transport>,
        config: SessionConfig,
        sink: Arc<dyn AdapterSink>,
    ) -> Result<Box<dyn AdapterSession>, AdapterError> {
        self.stats.starts.fetch_add(1, Ordering::SeqCst);
        sink.record(
            Direction::Host,
            json!({ "t": "hello", "cwd": config.cwd }).to_string(),
        );
        if self.fail_start {
            sink.process_exited(Ok(ProcessExit { code: Some(1) }));
            return Err(AdapterError::Protocol("the fake refused to start".into()));
        }
        sink.record(Direction::Backend, json!({ "t": "ready" }).to_string());
        Ok(Box::new(FakeSession {
            sink,
            stats: Arc::clone(&self.stats),
            inner: Mutex::default(),
        }))
    }

    fn projector(&self) -> Box<dyn Projector> {
        Box::new(FakeProjector {
            version: self.projector_version,
            turn: 0,
        })
    }
}

struct FakeSession {
    sink: Arc<dyn AdapterSink>,
    stats: Arc<Stats>,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    next_request: u32,
    pending: HashSet<String>,
    exited: bool,
}

impl FakeSession {
    fn backend(&self, line: Value) {
        self.sink.record(Direction::Backend, line.to_string());
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
    async fn send_user(&self, text: &str) -> Result<(), AdapterError> {
        self.running()?;
        self.sink.record(
            Direction::Host,
            json!({ "t": "user", "text": text }).to_string(),
        );
        if let Some(count) = text.strip_prefix("burst ") {
            let count: usize = count.parse().expect("burst <n>");
            for i in 0..count {
                self.backend(json!({ "t": "reply", "text": format!("{i}") }));
            }
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
            self.backend(json!({ "t": "usage", "tokens": text.len() * 10 }));
            self.backend(json!({ "t": "reply", "text": format!("echo: {text}") }));
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
        Ok(())
    }

    async fn stop(&self) -> Result<(), AdapterError> {
        self.stats.stops.fetch_add(1, Ordering::SeqCst);
        self.exit(Some(0));
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
                    }),
                ]
            }
            (Direction::Backend, Some("ready")) => vec![Event::SessionState(SessionState {
                state: LifecycleState::Running,
                reason: None,
            })],
            (Direction::Backend, Some("usage")) => {
                let tokens = value["tokens"].as_u64().unwrap_or(0);
                vec![Event::UsageUpdated(UsageUpdated {
                    context_tokens: Some(tokens),
                    context_window: Some(200_000),
                    cost_usd_total: Some(tokens as f64 / 1000.0),
                    ..UsageUpdated::default()
                })]
            }
            (Direction::Backend, Some("reply")) => vec![Event::AssistantText(AssistantText {
                block_id: format!("b-{}", line.index),
                text: text("text"),
                is_final: true,
            })],
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
