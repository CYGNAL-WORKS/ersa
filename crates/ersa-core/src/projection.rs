//! Projection: a session's events, made each time Core opens the session and
//! never stored (ADR 0017, PROTO-AGENT-EVENTS Projection rules).
//!
//! A [`View`] is one opening of one session. Its events come from three
//! inputs, numbered in one gap-free sequence:
//!
//! 1. The session record, the session's history, projected by the adapter
//!    ([`Adapter::read_record`](ersa_adapter::Adapter::read_record)) and
//!    handed to [`View::open`]. Its events come first.
//! 2. The live stream: each raw line Core receives from or writes to a
//!    running backend, projected by the adapter's projector
//!    ([`View::live`]). A message the record already had, identified by the
//!    backend's message id, yields its events once (PROTO-AGENT-EVENTS-R12).
//! 3. Core's own lifecycle facts about the backend process
//!    ([`View::lifecycle`]), which only Core emits (PROTO-AGENT-EVENTS-R13).
//!
//! It does no I/O, so opening the same record twice gives the same events
//! (PROTO-AGENT-EVENTS-R11). Running a session is `sessions`' job; reading
//! the record is the adapter's.

use std::collections::HashSet;

use ersa_adapter::{Projected, Projector, RawLine, RecordEvent};
use ersa_protocol::core_api::{PendingKind, PendingRequest, Tile};
use ersa_protocol::events::{
    BackendPassthrough, EndReason, Envelope, ErrorCode, ErrorEvent, Event, Input, LifecycleState,
    PROJECTION_VERSION, SessionEnded, SessionNotice, SessionState, Source,
};
use serde_json::Value;

use crate::app_log::LifecycleFact;

/// The `ts` of a record event whose entry gave none (a line that could not
/// be read) before any entry that did: fixed, so projecting again gives the
/// same value.
const NO_TIME: &str = "1970-01-01T00:00:00.000Z";

/// One opening of one session: its events so far, and what they add up to
/// (the tile, the state, the turn).
pub struct View {
    session_id: String,
    backend: String,
    /// The adapter's projector for live lines; `None` for a view that will
    /// never see any (a session not running in this Core).
    projector: Option<Box<dyn Projector>>,
    /// The version `source.backend_version` carries for live and Core
    /// events: from `process_started` until the backend announces its own.
    backend_version: String,
    /// The `ts` of the last event, for record events whose entry has none.
    last_ts: String,
    next_seq: u64,
    /// The message ids the record had: their live events are not repeated.
    record_ids: HashSet<String>,
    /// Whether `session.ended` has been emitted.
    ended: bool,
    tile: Tile,
    state: LifecycleState,
    turn: u32,
    backend_session_id: Option<String>,
}

impl View {
    /// Opens a view with the events of the session's record, numbered from 0.
    ///
    /// `projector` is the adapter's, for a session whose backend this Core is
    /// running; a view without one starts out `ended`, since nothing is
    /// running it.
    pub fn open(
        session_id: &str,
        backend: &str,
        projector: Option<Box<dyn Projector>>,
        record: &[RecordEvent],
    ) -> (View, Vec<Envelope>) {
        let state = if projector.is_some() {
            LifecycleState::Starting
        } else {
            LifecycleState::Ended
        };
        let mut view = View {
            session_id: session_id.to_string(),
            backend: backend.to_string(),
            projector,
            backend_version: String::new(),
            last_ts: NO_TIME.to_string(),
            next_seq: 0,
            record_ids: HashSet::new(),
            ended: false,
            tile: Tile {
                state,
                ..Tile::starting()
            },
            state,
            turn: 0,
            backend_session_id: None,
        };
        let mut out = Vec::with_capacity(record.len());
        for event in record {
            if let Some(id) = &event.message_id {
                view.record_ids.insert(id.clone());
            }
            let ts = event.ts.clone().unwrap_or_else(|| view.last_ts.clone());
            let source = Source {
                backend: view.backend.clone(),
                backend_version: event.backend_version.clone().unwrap_or_default(),
                input: Input::Record,
                message_id: event.message_id.clone(),
            };
            out.push(view.wrap(ts, source, event.event.clone()));
        }
        (view, out)
    }

    /// The events a live line completes. Every line yields at least one
    /// (PROTO-AGENT-EVENTS-R02), unless the record already had the message it
    /// carries.
    pub fn live(&mut self, line: &RawLine) -> Vec<Envelope> {
        let projected = self.project_live(line);
        let mut out = Vec::with_capacity(projected.len());
        for Projected { message_id, event } in projected {
            // In the record already, so shown already (PROTO-AGENT-EVENTS-R12).
            if message_id
                .as_ref()
                .is_some_and(|id| self.record_ids.contains(id))
            {
                continue;
            }
            // `session.started` carries the version it announces.
            if let Event::SessionStarted(started) = &event {
                self.backend_version = started.backend_version.clone();
            }
            let source = Source {
                backend: self.backend.clone(),
                backend_version: self.backend_version.clone(),
                input: Input::Live,
                message_id,
            };
            out.push(self.wrap(line.ts.clone(), source, event));
        }
        out
    }

    /// The events one of Core's lifecycle facts yields, learnt at `ts`.
    pub fn lifecycle(&mut self, fact: &LifecycleFact, ts: &str) -> Vec<Envelope> {
        let events = self.fact_events(fact);
        events
            .into_iter()
            .map(|event| {
                let source = Source {
                    backend: self.backend.clone(),
                    backend_version: self.backend_version.clone(),
                    input: Input::Core,
                    message_id: None,
                };
                self.wrap(ts.to_string(), source, event)
            })
            .collect()
    }

    /// A notice from Core itself, such as one that another process has the
    /// session open, learnt at `ts`.
    pub fn notice(&mut self, notice: SessionNotice, ts: &str) -> Envelope {
        let source = Source {
            backend: self.backend.clone(),
            backend_version: self.backend_version.clone(),
            input: Input::Core,
            message_id: None,
        };
        self.wrap(ts.to_string(), source, Event::SessionNotice(notice))
    }

    /// Numbers `event`, keeps the tile and state in step, and wraps it.
    fn wrap(&mut self, ts: String, source: Source, event: Event) -> Envelope {
        self.observe(&event);
        self.last_ts.clone_from(&ts);
        let envelope = Envelope {
            v: PROJECTION_VERSION,
            session_id: self.session_id.clone(),
            seq: self.next_seq,
            ts,
            source,
            event,
        };
        self.next_seq += 1;
        envelope
    }

    /// What the adapter's projector makes of a live line. A projector that
    /// panics on a line it was not built for must not take the session down
    /// with it: the line becomes a recoverable `error` and a passthrough
    /// (PROTO-AGENT-EVENTS, Errors and recovery), and projection carries on
    /// with the next line.
    fn project_live(&mut self, line: &RawLine) -> Vec<Projected> {
        let Some(projector) = self.projector.as_mut() else {
            return vec![unprojectable(&self.backend, line, "no projector")];
        };
        // `AssertUnwindSafe`: after a panic the projector may hold half an
        // update, which can only make later events less accurate, never
        // unsafe.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| projector.project(line))) {
            Ok(projected) => projected,
            Err(_) => {
                tracing::error!(index = line.index, "the projector panicked on a raw line");
                vec![
                    Projected::from_line(
                        line,
                        Event::Error(ErrorEvent {
                            code: ErrorCode::Protocol,
                            message: format!("raw line {} could not be projected", line.index),
                            recoverable: true,
                            action: None,
                        }),
                    ),
                    unprojectable(&self.backend, line, "unprojectable"),
                ]
            }
        }
    }

    /// Keeps the tile, state, turn, and backend facts in step with `event`.
    fn observe(&mut self, event: &Event) {
        let pending = &mut self.tile.pending;
        let mut remove = |request_id: &str| pending.retain(|p| p.request_id != request_id);
        match event {
            Event::SessionStarted(started) => {
                if started.backend_session_id.is_some() {
                    self.backend_session_id = started.backend_session_id.clone();
                }
            }
            Event::SessionState(state) => {
                self.state = state.state;
                self.tile.state = state.state;
            }
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

    /// The events a lifecycle fact yields.
    fn fact_events(&mut self, fact: &LifecycleFact) -> Vec<Event> {
        match fact {
            LifecycleFact::ProcessStarted {
                backend_version, ..
            } => {
                self.backend_version.clone_from(backend_version);
                vec![state(LifecycleState::Starting, None)]
            }
            LifecycleFact::ProcessExited {
                code,
                stopped_by_user,
                error,
            } => self.process_exited(*code, *stopped_by_user, error.clone()),
            LifecycleFact::StartFailed {
                message,
                code,
                stopped_by_user,
            } => {
                if *stopped_by_user {
                    if self.ended {
                        return vec![state(LifecycleState::Ended, None)];
                    }
                    return self.end(LifecycleState::Ended, EndReason::UserStopped, *code, None);
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
                        *code,
                        Some(message),
                    ));
                }
                out
            }
            LifecycleFact::CoreRestarted => {
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

    /// Drops the projector once the backend has gone: no more live lines
    /// come, and the projector's state is no longer needed.
    pub fn finish_live(&mut self) {
        self.projector = None;
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

    /// The backend's own id for the session, once its live stream named it.
    pub fn backend_session_id(&self) -> Option<&str> {
        self.backend_session_id.as_deref()
    }

    /// The latest title the backend announced on its live stream
    /// ([`Projector::announced_title`]); `None` before any, and once the
    /// live stream is over.
    pub fn announced_title(&self) -> Option<&str> {
        self.projector.as_ref()?.announced_title()
    }
}

fn state(state: LifecycleState, reason: Option<String>) -> Event {
    Event::SessionState(SessionState { state, reason })
}

/// A live line kept as it came, for one no projector could read.
fn unprojectable(backend: &str, line: &RawLine, kind: &str) -> Projected {
    let payload =
        serde_json::from_str(&line.line).unwrap_or_else(|_| Value::String(line.line.clone()));
    Projected::from_line(
        line,
        Event::BackendPassthrough(BackendPassthrough {
            backend: backend.to_string(),
            kind: kind.into(),
            payload,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ersa_adapter::Direction;
    use ersa_protocol::events::{
        AssistantText, MessageOrigin, PermissionDecided, UsageUpdated, UserMessage,
    };

    /// Echoes every live line as a passthrough, and a line `msg:<id>` as an
    /// assistant text of message `<id>`.
    struct Echo;

    impl Projector for Echo {
        fn version(&self) -> u32 {
            1
        }
        fn project(&mut self, line: &RawLine) -> Vec<Projected> {
            if let Some(id) = line.line.strip_prefix("msg:") {
                return vec![Projected::of_message(Some(id.into()), text(id))];
            }
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

    fn text(id: &str) -> Event {
        Event::AssistantText(AssistantText {
            block_id: format!("{id}/0"),
            text: id.into(),
            is_final: true,
        })
    }

    fn line(index: u64, dir: Direction, text: &str) -> RawLine {
        RawLine {
            index,
            ts: format!("2026-10-31T00:00:0{index}.000Z"),
            dir,
            line: text.to_string(),
        }
    }

    fn recorded(id: &str, ts: &str, event: Event) -> RecordEvent {
        RecordEvent {
            message_id: Some(id.into()),
            ts: Some(ts.into()),
            backend_version: Some("2.0".into()),
            event,
        }
    }

    fn types(envelopes: &[Envelope]) -> Vec<&'static str> {
        envelopes.iter().map(|e| e.event.type_name()).collect()
    }

    fn live_view() -> View {
        View::open("s-1", "echo", Some(Box::new(Echo)), &[]).0
    }

    // verifies: PROTO-AGENT-EVENTS-R01
    #[test]
    fn envelopes_are_numbered_without_gaps_record_first() {
        let record = vec![
            recorded(
                "u1",
                "2026-10-30T00:00:00.000Z",
                Event::UserMessage(UserMessage {
                    text: "hi".into(),
                    origin: MessageOrigin::Human,
                    peer: None,
                    attachments: Vec::new(),
                }),
            ),
            recorded("a1", "2026-10-30T00:00:01.000Z", text("a1")),
        ];
        let (mut view, mut events) = View::open("s-1", "echo", Some(Box::new(Echo)), &record);
        let started = LifecycleFact::ProcessStarted {
            backend: "echo".into(),
            backend_version: "1.0".into(),
        };
        events.extend(view.lifecycle(&started, "2026-10-31T00:00:00.000Z"));
        events.extend(view.live(&line(0, Direction::Host, "hello")));
        events.extend(view.live(&line(1, Direction::Backend, "world")));
        let exited = LifecycleFact::ProcessExited {
            code: Some(0),
            stopped_by_user: true,
            error: None,
        };
        events.extend(view.lifecycle(&exited, "2026-10-31T00:00:09.000Z"));

        let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, (0..events.len() as u64).collect::<Vec<_>>());
        let inputs: Vec<Input> = events.iter().map(|e| e.source.input).collect();
        use Input::{Core, Live, Record};
        assert_eq!(inputs, [Record, Record, Core, Live, Live, Core, Core]);
        for event in &events {
            assert_eq!(event.v, PROJECTION_VERSION);
            assert_eq!(event.session_id, "s-1");
        }
        // Record events carry the entry's time and version; live ones the
        // line's time and the version Core started the backend with.
        assert_eq!(events[0].ts, "2026-10-30T00:00:00.000Z");
        assert_eq!(events[0].source.backend_version, "2.0");
        assert_eq!(events[0].source.message_id.as_deref(), Some("u1"));
        assert_eq!(events[4].ts, "2026-10-31T00:00:01.000Z");
        assert_eq!(events[4].source.backend_version, "1.0");
        assert_eq!(types(&events[5..]), ["session.state", "session.ended"]);
        assert_eq!(view.last_seq(), Some(6));
    }

    // verifies: PROTO-AGENT-EVENTS-R11
    #[test]
    fn opening_the_same_record_twice_gives_the_same_events() {
        let record = || {
            vec![
                recorded("a1", "2026-10-30T00:00:01.000Z", text("a1")),
                // An unreadable line: no id, no time.
                RecordEvent {
                    message_id: None,
                    ts: None,
                    backend_version: None,
                    event: Event::Error(ErrorEvent {
                        code: ErrorCode::Protocol,
                        message: "bad".into(),
                        recoverable: true,
                        action: None,
                    }),
                },
            ]
        };
        let (_, first) = View::open("s", "echo", None, &record());
        let (_, second) = View::open("s", "echo", None, &record());
        assert_eq!(first, second);
        // The entry without a time takes the one before it.
        assert_eq!(first[1].ts, "2026-10-30T00:00:01.000Z");
    }

    // verifies: PROTO-AGENT-EVENTS-R13
    #[test]
    fn lifecycle_events_come_from_core_facts_only() {
        let mut crash = live_view();
        let exit = LifecycleFact::ProcessExited {
            code: Some(3),
            stopped_by_user: false,
            error: None,
        };
        let events = crash.lifecycle(&exit, "2026-10-31T00:00:00.000Z");
        assert_eq!(types(&events), ["error", "session.state", "session.ended"]);
        assert!(events.iter().all(|e| e.source.input == Input::Core));
        assert_eq!(crash.state(), LifecycleState::Error);

        let mut failed = live_view();
        let failure = |stopped_by_user| LifecycleFact::StartFailed {
            message: "no initialize answer".into(),
            code: Some(1),
            stopped_by_user,
        };
        let events = failed.lifecycle(&failure(false), "2026-10-31T00:00:00.000Z");
        assert_eq!(types(&events), ["error", "session.state", "session.ended"]);
        let Event::SessionEnded(ended) = &events[2].event else {
            unreachable!()
        };
        assert_eq!(ended.exit_code, Some(1));
        assert_eq!(ended.reason, EndReason::Error);

        // Stopped before it was ready: a stop, not an error.
        let mut stopped = live_view();
        let events = stopped.lifecycle(&failure(true), "2026-10-31T00:00:00.000Z");
        assert_eq!(types(&events), ["session.state", "session.ended"]);
        assert_eq!(stopped.state(), LifecycleState::Ended);

        // A record never yields lifecycle events, and a view of a session no
        // Core is running starts out ended without any.
        let (view, events) = View::open("s", "echo", None, &[]);
        assert!(events.is_empty());
        assert_eq!(view.state(), LifecycleState::Ended);
        assert_eq!(view.tile().state, LifecycleState::Ended);
    }

    // verifies: PROTO-CORE-API-R05
    #[test]
    fn the_tile_follows_usage_and_pending_requests() {
        let mut view = live_view();
        let usage = Event::UsageUpdated(UsageUpdated {
            context_tokens: Some(10),
            cost_usd_total: Some(0.5),
            ..UsageUpdated::default()
        });
        view.observe(&usage);
        view.observe(&Event::UsageUpdated(UsageUpdated {
            context_tokens: Some(20),
            ..UsageUpdated::default()
        }));
        view.add_pending(PendingKind::Permission, "r1");
        view.add_pending(PendingKind::Question, "q1");
        view.observe(&Event::PermissionDecided(PermissionDecided {
            request_id: "r1".into(),
            behavior: ersa_protocol::events::PermissionBehavior::Allow,
            decided_by: ersa_protocol::events::DecidedBy::User,
            message: None,
            applied_suggestions: None,
        }));
        let tile = view.tile();
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
