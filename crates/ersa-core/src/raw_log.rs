//! Raw log and projection: every line a backend emits, every line the host
//! writes to it, and Core's own lifecycle records are stored verbatim per
//! session (ADR 0010). Ersa events are a versioned projection derived from
//! the raw log, cached, and re-derived when the projection version changes.
//! Transcript, rail state, and resume are all projections.
//!
//! This module is the pure part: what Core's lifecycle records look like
//! ([`CoreRecord`]), and [`Projection`], which turns one session's raw lines
//! into envelopes. It does no I/O, so projecting the same log twice gives the
//! same events (PROTO-AGENT-EVENTS-R03, ARCH-CORE-R06). Storing lines and
//! events is `persistence`'s job; running a live session is `sessions`'.

use ersa_adapter::{Direction, Projector, RawLine};
use ersa_protocol::core_api::{PendingKind, PendingRequest, Tile};
use ersa_protocol::events::{
    BackendPassthrough, EndReason, Envelope, ErrorCode, ErrorEvent, Event, LifecycleState,
    PROJECTION_VERSION, SessionEnded, SessionState, Source,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A lifecycle record Core writes to a session's raw log, as a `core` line.
///
/// A small JSON shape of Ersa's own, tagged by `type`, such as
/// `{"type":"process_exited","code":0}`. Core projects these itself
/// (`session.state`, `session.ended`, `error`); the adapter's projector never
/// sees them. Fields are only ever added, with defaults, so every record
/// ever written keeps parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreRecord {
    /// Core is starting the backend process.
    ProcessStarted {
        backend: String,
        /// The version detection found, or empty when unknown. The backend's
        /// own announcement (`session.started`) replaces it.
        #[serde(default)]
        backend_version: String,
    },
    /// The backend process ended.
    ProcessExited {
        /// The exit code; absent when there was none (killed) or the
        /// transport lost track of the process.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<i32>,
        /// The user had asked for the session to stop.
        #[serde(default, skip_serializing_if = "is_false")]
        stopped_by_user: bool,
        /// Why the exit is unknown, when the transport lost track of it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// The adapter did not finish starting the backend. Ends the session;
    /// when the process had already exited, this record carries its code
    /// instead of a `process_exited` of its own.
    StartFailed {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<i32>,
        /// The user stopped the session before it was ready, which is a
        /// stop, not a failure.
        #[serde(default, skip_serializing_if = "is_false")]
        stopped_by_user: bool,
    },
    /// Core found the session marked live when it started: the Core that ran
    /// it ended without recording the backend's exit.
    CoreRestarted,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl CoreRecord {
    /// The record as one raw-log line.
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("core records always serialize")
    }
}

/// One session's projection in progress: the adapter's projector for
/// `backend` and `host` lines, Core's own rules for `core` lines, and what
/// the events so far add up to (the tile, the state, the turn).
///
/// Feed it the raw log from the first line, in order. A fresh one over the
/// same log yields the same envelopes with the same `seq`.
pub struct Projection {
    session_id: String,
    backend: String,
    projector: Box<dyn Projector>,
    /// The version `source.backend_version` carries: from the
    /// `process_started` record until the backend announces its own.
    backend_version: String,
    next_seq: u64,
    /// Whether `session.ended` has been emitted.
    ended: bool,
    tile: Tile,
    state: LifecycleState,
    turn: u32,
    backend_session_id: Option<String>,
}

impl Projection {
    pub fn new(session_id: &str, backend: &str, projector: Box<dyn Projector>) -> Self {
        Projection {
            session_id: session_id.to_string(),
            backend: backend.to_string(),
            projector,
            backend_version: String::new(),
            next_seq: 0,
            ended: false,
            tile: Tile::default(),
            state: LifecycleState::Starting,
            turn: 0,
            backend_session_id: None,
        }
    }

    /// The adapter projector's version, half of the cache key next to `v`.
    pub fn projector_version(&self) -> u32 {
        self.projector.version()
    }

    /// The envelopes `line` completes, numbered from where the last call
    /// left off. Every line yields at least one (PROTO-AGENT-EVENTS-R02).
    pub fn project(&mut self, line: &RawLine) -> Vec<Envelope> {
        let projected: Vec<(u64, Event)> = match line.dir {
            Direction::Core => self
                .core_line(line)
                .into_iter()
                .map(|event| (line.index, event))
                .collect(),
            Direction::Backend | Direction::Host => self.adapter_line(line),
        };
        let mut out = Vec::with_capacity(projected.len());
        for (raw_from, event) in projected {
            self.observe(&event);
            out.push(Envelope {
                v: PROJECTION_VERSION,
                session_id: self.session_id.clone(),
                seq: self.next_seq,
                // From the raw log, never the clock (PROTO-AGENT-EVENTS, Envelope).
                ts: line.ts.clone(),
                source: Source {
                    backend: self.backend.clone(),
                    backend_version: self.backend_version.clone(),
                    // A projector may name a later line by mistake; the range
                    // must still be ordered.
                    raw_from: raw_from.min(line.index),
                    raw_to: line.index,
                },
                event,
            });
            self.next_seq += 1;
        }
        out
    }

    /// What the adapter's projector makes of a backend or host line. A
    /// projector that panics on a line it was not built for must not take
    /// the session down with it: the line becomes a recoverable `error` and
    /// a passthrough (PROTO-AGENT-EVENTS, Errors and recovery), and
    /// projection carries on with the next line.
    fn adapter_line(&mut self, line: &RawLine) -> Vec<(u64, Event)> {
        // `AssertUnwindSafe`: after a panic the projector may hold half an
        // update, which can only make later events less accurate, never
        // unsafe.
        let projector = &mut self.projector;
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| projector.project(line))) {
            Ok(projected) => projected
                .into_iter()
                .map(|p| (p.raw_from, p.event))
                .collect(),
            Err(_) => {
                tracing::error!(index = line.index, "the projector panicked on a raw line");
                let payload = serde_json::from_str(&line.line)
                    .unwrap_or_else(|_| Value::String(line.line.clone()));
                vec![
                    (
                        line.index,
                        Event::Error(ErrorEvent {
                            code: ErrorCode::Protocol,
                            message: format!("raw line {} could not be projected", line.index),
                            recoverable: true,
                            action: None,
                        }),
                    ),
                    (
                        line.index,
                        Event::BackendPassthrough(BackendPassthrough {
                            backend: self.backend.clone(),
                            kind: "unprojectable".into(),
                            payload,
                        }),
                    ),
                ]
            }
        }
    }

    /// Keeps the tile, state, turn, and backend facts in step with `event`.
    /// Runs before the event is wrapped, so `session.started` already carries
    /// the version it announces.
    fn observe(&mut self, event: &Event) {
        let pending = &mut self.tile.pending;
        let mut remove = |request_id: &str| pending.retain(|p| p.request_id != request_id);
        match event {
            Event::SessionStarted(started) => {
                self.backend_version = started.backend_version.clone();
                if started.backend_session_id.is_some() {
                    self.backend_session_id = started.backend_session_id.clone();
                }
            }
            Event::SessionState(state) => self.state = state.state,
            Event::TurnStarted(started) => self.turn = started.turn,
            Event::UsageUpdated(usage) => {
                // Absent means "no new information": keep the last value.
                if usage.context_tokens.is_some() {
                    self.tile.context_tokens = usage.context_tokens;
                }
                if usage.context_window.is_some() {
                    self.tile.context_window = usage.context_window;
                }
                if usage.cost_usd_total.is_some() {
                    self.tile.cost_usd_total = usage.cost_usd_total;
                }
            }
            Event::PermissionRequested(asked) => {
                self.add_pending(PendingKind::Permission, &asked.request_id);
            }
            Event::PermissionDecided(decided) => remove(&decided.request_id),
            Event::QuestionAsked(asked) => {
                self.add_pending(PendingKind::Question, &asked.request_id)
            }
            Event::QuestionAnswered(answered) => remove(&answered.request_id),
            // A backend that has gone is waiting on nobody.
            Event::SessionEnded(_) => self.tile.pending.clear(),
            _ => {}
        }
    }

    fn add_pending(&mut self, kind: PendingKind, request_id: &str) {
        if !self.tile.pending.iter().any(|p| p.request_id == request_id) {
            self.tile.pending.push(PendingRequest {
                kind,
                request_id: request_id.to_string(),
            });
        }
    }

    /// The events one of Core's own lifecycle lines yields.
    fn core_line(&mut self, line: &RawLine) -> Vec<Event> {
        let record = match serde_json::from_str::<CoreRecord>(&line.line) {
            Ok(record) => record,
            // Only Core writes these lines, so this means a record from a
            // newer Core. Keep it visible rather than drop it
            // (PROTO-AGENT-EVENTS, Errors and recovery).
            Err(e) => {
                let payload = serde_json::from_str(&line.line)
                    .unwrap_or_else(|_| Value::String(line.line.clone()));
                return vec![
                    Event::Error(ErrorEvent {
                        code: ErrorCode::Protocol,
                        message: format!(
                            "unreadable lifecycle record at raw line {}: {e}",
                            line.index
                        ),
                        recoverable: true,
                        action: None,
                    }),
                    Event::BackendPassthrough(BackendPassthrough {
                        backend: self.backend.clone(),
                        kind: "core/unknown".into(),
                        payload,
                    }),
                ];
            }
        };
        match record {
            CoreRecord::ProcessStarted {
                backend_version, ..
            } => {
                self.backend_version = backend_version;
                vec![state(LifecycleState::Starting, None)]
            }
            CoreRecord::ProcessExited {
                code,
                stopped_by_user,
                error,
            } => self.process_exited(code, stopped_by_user, error),
            CoreRecord::StartFailed {
                message,
                code,
                stopped_by_user,
            } => {
                if stopped_by_user {
                    if self.ended {
                        return vec![state(LifecycleState::Ended, None)];
                    }
                    return self.end(LifecycleState::Ended, EndReason::UserStopped, code, None);
                }
                let message = format!("the backend could not be started: {message}");
                let mut out = vec![Event::Error(ErrorEvent {
                    code: ErrorCode::BackendError,
                    message: message.clone(),
                    recoverable: false,
                    action: None,
                })];
                if !self.ended {
                    out.extend(self.end(
                        LifecycleState::Error,
                        EndReason::Error,
                        code,
                        Some(message),
                    ));
                }
                out
            }
            CoreRecord::CoreRestarted => {
                if self.ended {
                    return vec![state(LifecycleState::Ended, None)];
                }
                let reason = "Core restarted while the session was running".to_string();
                self.end(LifecycleState::Ended, EndReason::Error, None, Some(reason))
            }
        }
    }

    fn process_exited(
        &mut self,
        code: Option<i32>,
        stopped_by_user: bool,
        error: Option<String>,
    ) -> Vec<Event> {
        if self.ended {
            return vec![state(LifecycleState::Ended, None)];
        }
        if stopped_by_user {
            return self.end(LifecycleState::Ended, EndReason::UserStopped, code, None);
        }
        if code == Some(0) {
            return self.end(LifecycleState::Ended, EndReason::BackendExited, code, None);
        }
        // An exit nobody asked for is something the user should see
        // (ARCH-CORE, Errors).
        let message = match (&error, code) {
            (Some(error), _) => format!("lost track of the backend process: {error}"),
            (None, Some(code)) => format!("the backend exited with code {code}"),
            (None, None) => "the backend was killed".to_string(),
        };
        let mut out = vec![Event::Error(ErrorEvent {
            code: ErrorCode::BackendExited,
            message: message.clone(),
            recoverable: false,
            action: None,
        })];
        let reason = if error.is_some() {
            EndReason::Error
        } else {
            EndReason::BackendExited
        };
        out.extend(self.end(LifecycleState::Error, reason, code, Some(message)));
        out
    }

    /// The last two events of a session: its final state, then `session.ended`.
    fn end(
        &mut self,
        final_state: LifecycleState,
        reason: EndReason,
        exit_code: Option<i32>,
        why: Option<String>,
    ) -> Vec<Event> {
        self.ended = true;
        vec![
            state(final_state, why),
            Event::SessionEnded(SessionEnded { reason, exit_code }),
        ]
    }

    /// The `seq` of the last event so far; `None` before the first.
    pub fn last_seq(&self) -> Option<u64> {
        self.next_seq.checked_sub(1)
    }

    pub fn tile(&self) -> &Tile {
        &self.tile
    }

    pub fn state(&self) -> LifecycleState {
        self.state
    }

    /// The number of the current or last turn; 0 before the first.
    pub fn turn(&self) -> u32 {
        self.turn
    }

    pub fn backend_session_id(&self) -> Option<&str> {
        self.backend_session_id.as_deref()
    }

    /// Whether the session has ended.
    pub fn ended(&self) -> bool {
        self.ended
    }
}

fn state(state: LifecycleState, reason: Option<String>) -> Event {
    Event::SessionState(SessionState { state, reason })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ersa_adapter::Projected;
    use ersa_protocol::events::{PermissionDecided, UsageUpdated};

    /// Echoes every adapter line as a passthrough.
    struct Echo;

    impl Projector for Echo {
        fn version(&self) -> u32 {
            1
        }
        fn project(&mut self, line: &RawLine) -> Vec<Projected> {
            vec![Projected::from_line(
                line,
                Event::BackendPassthrough(BackendPassthrough {
                    backend: "echo".into(),
                    kind: "line".into(),
                    payload: Value::String(line.line.clone()),
                }),
            )]
        }
    }

    fn line(index: u64, dir: Direction, text: &str) -> RawLine {
        RawLine {
            index,
            ts: format!("2026-10-31T00:00:0{index}.000Z"),
            dir,
            line: text.to_string(),
        }
    }

    fn core(index: u64, record: CoreRecord) -> RawLine {
        line(index, Direction::Core, &record.to_line())
    }

    fn types(envelopes: &[Envelope]) -> Vec<&'static str> {
        envelopes.iter().map(|e| e.event.type_name()).collect()
    }

    // verifies: PROTO-AGENT-EVENTS-R01
    #[test]
    fn envelopes_are_numbered_without_gaps_and_stamped_from_the_log() {
        let mut projection = Projection::new("s-1", "echo", Box::new(Echo));
        let log = [
            core(
                0,
                CoreRecord::ProcessStarted {
                    backend: "echo".into(),
                    backend_version: "1.0".into(),
                },
            ),
            line(1, Direction::Host, "hello"),
            line(2, Direction::Backend, "world"),
            core(
                3,
                CoreRecord::ProcessExited {
                    code: Some(0),
                    stopped_by_user: true,
                    error: None,
                },
            ),
        ];
        let events: Vec<Envelope> = log.iter().flat_map(|l| projection.project(l)).collect();
        let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, (0..events.len() as u64).collect::<Vec<_>>());
        for event in &events {
            assert_eq!(event.v, PROJECTION_VERSION);
            assert_eq!(event.session_id, "s-1");
            assert_eq!(event.source.backend_version, "1.0");
            assert_eq!(event.ts, log[event.source.raw_to as usize].ts);
        }
        assert_eq!(
            types(&events),
            [
                "session.state",
                "backend.passthrough",
                "backend.passthrough",
                "session.state",
                "session.ended"
            ]
        );
        assert_eq!(projection.last_seq(), Some(4));
        assert!(projection.ended());
    }

    #[test]
    fn a_crash_is_an_error_and_a_failed_start_ends_once() {
        let mut crash = Projection::new("s", "echo", Box::new(Echo));
        let exit = CoreRecord::ProcessExited {
            code: Some(3),
            stopped_by_user: false,
            error: None,
        };
        let events = crash.project(&core(0, exit));
        assert_eq!(types(&events), ["error", "session.state", "session.ended"]);
        assert_eq!(crash.state(), LifecycleState::Error);

        let mut failed = Projection::new("s", "echo", Box::new(Echo));
        let failure = |stopped_by_user| CoreRecord::StartFailed {
            message: "no initialize answer".into(),
            code: Some(1),
            stopped_by_user,
        };
        let events = failed.project(&core(0, failure(false)));
        assert_eq!(types(&events), ["error", "session.state", "session.ended"]);
        let Event::SessionEnded(ended) = &events[2].event else {
            unreachable!()
        };
        assert_eq!(ended.exit_code, Some(1));
        assert_eq!(ended.reason, EndReason::Error);

        // Stopped before it was ready: a stop, not an error.
        let mut stopped = Projection::new("s", "echo", Box::new(Echo));
        let events = stopped.project(&core(0, failure(true)));
        assert_eq!(types(&events), ["session.state", "session.ended"]);
        assert_eq!(stopped.state(), LifecycleState::Ended);
    }

    #[test]
    fn an_unreadable_core_record_is_reported_and_kept() {
        let mut projection = Projection::new("s", "echo", Box::new(Echo));
        let events =
            projection.project(&line(0, Direction::Core, "{\"type\":\"from_the_future\"}"));
        assert_eq!(types(&events), ["error", "backend.passthrough"]);
    }

    // verifies: PROTO-CORE-API-R05
    #[test]
    fn the_tile_follows_usage_and_pending_requests() {
        let mut projection = Projection::new("s", "echo", Box::new(Echo));
        let usage = Event::UsageUpdated(UsageUpdated {
            context_tokens: Some(10),
            cost_usd_total: Some(0.5),
            ..UsageUpdated::default()
        });
        projection.observe(&usage);
        projection.observe(&Event::UsageUpdated(UsageUpdated {
            context_tokens: Some(20),
            ..UsageUpdated::default()
        }));
        projection.add_pending(PendingKind::Permission, "r1");
        projection.add_pending(PendingKind::Question, "q1");
        projection.observe(&Event::PermissionDecided(PermissionDecided {
            request_id: "r1".into(),
            behavior: ersa_protocol::events::PermissionBehavior::Allow,
            decided_by: ersa_protocol::events::DecidedBy::User,
            message: None,
            applied_suggestions: None,
        }));
        let tile = projection.tile();
        assert_eq!(tile.context_tokens, Some(20));
        // An absent field keeps the last value.
        assert_eq!(tile.cost_usd_total, Some(0.5));
        assert_eq!(
            tile.pending,
            [PendingRequest {
                kind: PendingKind::Question,
                request_id: "q1".into()
            }]
        );
    }
}
