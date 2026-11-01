//! The projection from a Claude Code session's raw log to Ersa events
//! (PROTO-AGENT-EVENTS, Projection rules).
//!
//! The projector reads both directions of the log: what the CLI wrote
//! (`backend` lines) and what the adapter wrote to it (`host` lines). The host
//! lines matter as much as the CLI's: a user message only exists in the log as
//! the host line that sent it, and the user's permission answer only as the
//! host's `control_response`.
//!
//! It is a pure function of the lines it is fed, in order. The state it keeps
//! between lines (the turn number, text streamed so far, which requests are
//! pending) is all derived from earlier lines.

use std::collections::{HashMap, HashSet};

use ersa_adapter::{Direction, Projected, Projector, RawLine};
use ersa_protocol::events::{
    AssistantText, AssistantThinking, BackendPassthrough, DecidedBy, ErrorAction, ErrorCode,
    ErrorEvent, Event, LifecycleState, MessageOrigin, NoticeCode, NoticeLevel, PermissionBehavior,
    PermissionDecided, PermissionDeniedUnanswered, PermissionFlags, PermissionReason,
    PermissionRequested, PermissionSuggestion, RateLimit, SessionNotice, SessionStarted,
    SessionState, ToolRequested, ToolResult, ToolResultContent, TurnCompleted, TurnOutcome,
    TurnStarted, TurnTrigger, UsageUpdated, UserMessage,
};
use serde_json::Value;

use crate::suggestions;
use crate::wire::{
    AssistantMessage, CanUseTool, CliMessage, CliRequest, CliResponse, ContentBlock, Delta,
    InitializeResponse, MessageContent, RateLimitInfo, ResultMessage, StreamEvent,
    StreamEventMessage, SystemInit, SystemMessage, Usage,
};
use crate::{BACKEND, MAX_TESTED_VERSION};

/// The version of this projection. Bump it whenever the events produced for
/// some raw log would change, so Core re-projects cached sessions.
pub const PROJECTOR_VERSION: u32 = 1;

/// Projects one Claude Code session's raw log. Make a fresh one per pass.
#[derive(Debug, Default)]
pub struct ClaudeProjector {
    /// Whether `session.started` has been emitted (from the first `system/init`).
    session_started: bool,
    /// The number of the current or last turn; 0 before the first.
    turn: u32,
    turn_open: bool,
    /// `request_id`s of the host's `initialize` requests not yet answered.
    initialize_ids: HashSet<String>,
    /// Pending `can_use_tool` requests, by `request_id`, with the suggestions
    /// their `permission.requested` offered (to map an answer back to indices).
    permissions: HashMap<String, Vec<(usize, PermissionSuggestion)>>,
    /// How many content blocks the `assistant` lines of each message id have
    /// carried so far, which gives the next block's index in its message.
    blocks_seen: HashMap<String, u64>,
    /// Text streamed so far for blocks not yet completed, by block id.
    streaming: HashMap<String, StreamedText>,
    /// The message id from the last `message_start`, for stream events that
    /// do not name their message.
    stream_message: Option<String>,
    /// The model of the latest main-thread reply; picks the context window
    /// out of `result.modelUsage`, which lists subagent models too.
    main_model: Option<String>,
}

#[derive(Debug)]
struct StreamedText {
    /// The raw line the block's streaming started on.
    first_raw: u64,
    text: String,
}

impl ClaudeProjector {
    pub fn new() -> Self {
        ClaudeProjector::default()
    }
}

impl Projector for ClaudeProjector {
    fn version(&self) -> u32 {
        PROJECTOR_VERSION
    }

    fn project(&mut self, line: &RawLine) -> Vec<Projected> {
        let value: Value = match serde_json::from_str(&line.line) {
            Ok(value) => value,
            Err(e) => return unparsable(line, format!("raw line {} is not JSON: {e}", line.index)),
        };
        match line.dir {
            Direction::Backend => self.backend(line, value),
            Direction::Host => self.host(line, value),
            // Core projects its own lifecycle records; should the raw log's
            // direction gain such a variant, those lines are not ours.
            #[allow(unreachable_patterns)]
            _ => Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Backend lines

impl ClaudeProjector {
    fn backend(&mut self, line: &RawLine, value: Value) -> Vec<Projected> {
        // `CliMessage` keeps any line it cannot type as `Unknown`, so this
        // never fails; the fallback only satisfies the type checker.
        let message = serde_json::from_value(value.clone())
            .unwrap_or_else(|_| CliMessage::Unknown(value.clone()));
        if let Some(kind) = misparsed(&message) {
            // A kind the wire models but whose fields did not fit: the CLI has
            // changed shape. Say so, and keep the line (PROTO-AGENT-EVENTS-R10).
            return error_and_passthrough(
                line,
                format!(
                    "raw line {}: a {kind} line did not have the expected shape",
                    line.index
                ),
                value,
            );
        }

        match message {
            CliMessage::System(SystemMessage::Init(init)) => self.init(line, &init, value),
            CliMessage::System(SystemMessage::PermissionDenied(denied)) => {
                let mut out = self.ensure_turn(line);
                out.push(at(
                    line,
                    Event::PermissionDeniedUnanswered(PermissionDeniedUnanswered {
                        tool_use_id: denied.tool_use_id,
                        tool_name: denied.tool_name,
                        message: denied.message,
                    }),
                ));
                out
            }
            CliMessage::Assistant(reply) => self.assistant(line, reply, value),
            CliMessage::User(user) => {
                self.tool_results(line, user.message.content, user.is_replay, value)
            }
            CliMessage::StreamEvent(stream) => self.stream_event(line, stream, value),
            CliMessage::Result(result) => self.result(line, &result),
            CliMessage::RateLimitEvent(event) => vec![at(
                line,
                Event::UsageUpdated(UsageUpdated {
                    rate_limit: Some(rate_limit(&event.rate_limit_info)),
                    ..UsageUpdated::default()
                }),
            )],
            CliMessage::ControlRequest(request) => match request.request {
                CliRequest::CanUseTool(ask) => {
                    self.permission_requested(line, request.request_id, &ask, value)
                }
                CliRequest::Other(_) => vec![passthrough(line, value)],
            },
            CliMessage::ControlResponse(answer) => {
                self.control_response(line, answer.response, value)
            }
            CliMessage::ControlCancelRequest(cancel) => {
                if self.permissions.remove(&cancel.request_id).is_none() {
                    return vec![passthrough(line, value)];
                }
                // The CLI withdrew a prompt it was waiting on (after an
                // interrupt, say): the tool will not run, and nobody asked
                // the user, so the backend decided.
                vec![
                    at(
                        line,
                        Event::PermissionDecided(PermissionDecided {
                            request_id: cancel.request_id,
                            behavior: PermissionBehavior::Deny,
                            decided_by: DecidedBy::Backend,
                            message: None,
                            applied_suggestions: None,
                        }),
                    ),
                    state(line, LifecycleState::Running),
                ]
            }
            // Status, summaries, thinking-token estimates, keep-alives, and
            // anything this version does not know.
            _ => vec![passthrough(line, value)],
        }
    }

    fn init(&mut self, line: &RawLine, init: &SystemInit, value: Value) -> Vec<Projected> {
        // The CLI repeats `init` at the start of every turn; only the first
        // announces the session (PROTO-CLAUDE-CODE-STREAM-R06).
        if self.session_started {
            return vec![passthrough(line, value)];
        }
        self.session_started = true;
        vec![at(
            line,
            Event::SessionStarted(SessionStarted {
                backend: BACKEND.into(),
                backend_version: init.claude_code_version.clone(),
                model: init.model.clone(),
                cwd: init.cwd.clone(),
                capabilities: init.capabilities.clone(),
                backend_session_id: Some(init.session_id.clone()),
            }),
        )]
    }

    fn assistant(
        &mut self,
        line: &RawLine,
        reply: AssistantMessage,
        value: Value,
    ) -> Vec<Projected> {
        let mut out = self.ensure_turn(line);

        // A synthetic message carrying an error is the CLI telling Ersa
        // something went wrong, not the model speaking: it becomes a session
        // error and never transcript text (PROTO-CLAUDE-CODE-STREAM-R05).
        if let Some(error) = &reply.error {
            let text = plain_text(&reply.message.content);
            let event = if reply.is_authentication_failure() {
                ErrorEvent {
                    code: ErrorCode::AuthenticationFailed,
                    message: text.unwrap_or_else(|| "Claude Code is not logged in.".into()),
                    // The CLI keeps running; logging in and sending again works.
                    recoverable: true,
                    action: Some(ErrorAction::LogIn),
                }
            } else {
                ErrorEvent {
                    code: ErrorCode::BackendError,
                    message: text.unwrap_or_else(|| error.clone()),
                    recoverable: true,
                    action: None,
                }
            };
            out.push(at(line, Event::Error(event)));
            return out;
        }

        if reply.parent_tool_use_id.is_none() {
            self.main_model = Some(reply.message.model.clone());
        }
        let before = out.len();
        let message_id = reply.message.id;
        for block in reply.message.content {
            let block_id = self.next_block_id(&message_id);
            match block {
                ContentBlock::Text { text, .. } => {
                    // A streamed block's final event covers its whole stream.
                    let raw_from = self
                        .streaming
                        .remove(&block_id)
                        .map_or(line.index, |streamed| streamed.first_raw);
                    out.push(Projected {
                        raw_from,
                        event: Event::AssistantText(AssistantText {
                            block_id,
                            text,
                            is_final: true,
                        }),
                    });
                }
                ContentBlock::Thinking { thinking, .. } => out.push(at(
                    line,
                    Event::AssistantThinking(AssistantThinking {
                        block_id,
                        // Usually empty: the CLI withholds the text.
                        summary: (!thinking.is_empty()).then_some(thinking),
                        is_final: true,
                    }),
                )),
                ContentBlock::ToolUse {
                    id, name, input, ..
                } => match input {
                    Value::Object(input) => out.push(at(
                        line,
                        Event::ToolRequested(ToolRequested {
                            tool_use_id: id,
                            name,
                            display_name: None,
                            input,
                            parent_tool_use_id: reply.parent_tool_use_id.clone(),
                        }),
                    )),
                    _ => out.push(protocol_error(
                        line,
                        format!(
                            "raw line {}: tool_use {id} has an input that is not an object",
                            line.index
                        ),
                    )),
                },
                // A tool result never comes from the assistant; any other block
                // kind is new. The passthrough below keeps the line.
                ContentBlock::ToolResult { .. } | ContentBlock::Other(_) => {}
            }
        }
        if out.len() == before {
            out.push(passthrough(line, value));
        }
        out
    }

    /// The id of the next content block of message `message_id`. The CLI
    /// writes one `assistant` line per block, in order, so counting the blocks
    /// seen gives the block's index, which is also the index its stream events
    /// carry.
    fn next_block_id(&mut self, message_id: &str) -> String {
        let seen = self.blocks_seen.entry(message_id.to_string()).or_insert(0);
        let index = *seen;
        *seen += 1;
        block_id(message_id, index)
    }

    /// A `user` line: tool results the CLI produced, or (with
    /// `--replay-user-messages`) an echo of a host message.
    fn tool_results(
        &mut self,
        line: &RawLine,
        content: MessageContent,
        is_replay: Option<bool>,
        value: Value,
    ) -> Vec<Projected> {
        // An echo repeats a host line that already projected to `user.message`.
        let MessageContent::Blocks(blocks) = content else {
            return vec![passthrough(line, value)];
        };
        if is_replay == Some(true) {
            return vec![passthrough(line, value)];
        }
        let results: Vec<Event> = blocks
            .into_iter()
            .filter_map(|block| match block {
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                    ..
                } => Some(Event::ToolResult(ToolResult {
                    tool_use_id,
                    content: tool_output(content),
                    is_error: is_error.unwrap_or(false),
                    duration_ms: None,
                })),
                _ => None,
            })
            .collect();
        if results.is_empty() {
            return vec![passthrough(line, value)];
        }
        let mut out = self.ensure_turn(line);
        out.extend(results.into_iter().map(|event| at(line, event)));
        out
    }

    fn stream_event(
        &mut self,
        line: &RawLine,
        stream: StreamEventMessage,
        value: Value,
    ) -> Vec<Projected> {
        let message_id = stream
            .extra
            .get("api_message_id")
            .and_then(Value::as_str)
            .map(String::from)
            .or_else(|| self.stream_message.clone());
        match stream.event {
            StreamEvent::MessageStart { message } => {
                self.stream_message = Some(message.id);
            }
            StreamEvent::ContentBlockStart {
                index,
                content_block: ContentBlock::Text { text, .. },
            } => {
                if let Some(message_id) = message_id {
                    self.streaming.insert(
                        block_id(&message_id, index),
                        StreamedText {
                            first_raw: line.index,
                            text,
                        },
                    );
                }
            }
            StreamEvent::ContentBlockDelta {
                index,
                delta: Delta::TextDelta { text },
            } => {
                if let Some(message_id) = message_id {
                    // Text so far, growing with each delta; the complete
                    // `assistant` line ends the block with `final: true`
                    // (PROTO-AGENT-EVENTS-R07).
                    let block_id = block_id(&message_id, index);
                    let streamed = self
                        .streaming
                        .entry(block_id.clone())
                        .or_insert(StreamedText {
                            first_raw: line.index,
                            text: String::new(),
                        });
                    streamed.text.push_str(&text);
                    let event = Projected {
                        raw_from: streamed.first_raw,
                        event: Event::AssistantText(AssistantText {
                            block_id,
                            text: streamed.text.clone(),
                            is_final: false,
                        }),
                    };
                    let mut out = self.ensure_turn(line);
                    out.push(event);
                    return out;
                }
            }
            _ => {}
        }
        vec![passthrough(line, value)]
    }

    fn result(&mut self, line: &RawLine, result: &ResultMessage) -> Vec<Projected> {
        let mut out = self.ensure_turn(line);
        out.push(at(
            line,
            Event::TurnCompleted(TurnCompleted {
                turn: self.turn,
                outcome: outcome(result),
                duration_ms: result.duration_ms,
                is_error: result.is_error,
                // On an error `result` repeats the error text, which the error
                // event already carries.
                final_text: result.result.clone().filter(|_| !result.is_error),
            }),
        ));
        // PROTO-CLAUDE-CODE-STREAM-R08: cost from `total_cost_usd` (a running
        // total for the session), context from `usage`, window from
        // `modelUsage`.
        let window = self
            .main_model
            .as_ref()
            .and_then(|model| result.model_usage.get(model))
            .and_then(|usage| usage.context_window)
            .or_else(|| {
                result
                    .model_usage
                    .values()
                    .filter_map(|u| u.context_window)
                    .max()
            });
        out.push(at(
            line,
            Event::UsageUpdated(UsageUpdated {
                context_tokens: context_tokens(&result.usage),
                context_window: window,
                cost_usd_total: Some(result.total_cost_usd),
                turns: Some(self.turn),
                rate_limit: None,
            }),
        ));
        out.push(state(line, LifecycleState::Idle));

        // Message ids and streams never span turns.
        self.turn_open = false;
        self.blocks_seen.clear();
        self.streaming.clear();
        self.stream_message = None;
        out
    }

    fn permission_requested(
        &mut self,
        line: &RawLine,
        request_id: String,
        ask: &CanUseTool,
        value: Value,
    ) -> Vec<Projected> {
        let Value::Object(input) = &ask.input else {
            return error_and_passthrough(
                line,
                format!(
                    "raw line {}: can_use_tool input is not an object",
                    line.index
                ),
                value,
            );
        };
        let offered = suggestions::offered(&ask.permission_suggestions);
        let event = PermissionRequested {
            request_id: request_id.clone(),
            tool_use_id: ask.tool_use_id.clone(),
            tool_name: ask.tool_name.clone(),
            display_name: ask.display_name.clone(),
            description: ask.description.clone(),
            input: input.clone(),
            blocked_path: ask.blocked_path.clone(),
            suggestions: offered.iter().map(|(_, s)| s.clone()).collect(),
            flags: PermissionFlags {
                default_to_no: ask.default_to_no.unwrap_or(false),
                suppress_always_allow: ask.suppress_always_allow_rule.unwrap_or(false),
                requires_user_interaction: ask.requires_user_interaction.unwrap_or(false),
                classifier_approvable: ask.classifier_approvable.unwrap_or(false),
            },
            reason: ask.decision_reason.as_ref().map(|text| PermissionReason {
                kind: ask
                    .decision_reason_type
                    .clone()
                    .unwrap_or_else(|| "unspecified".into()),
                text: strip_ansi(text),
            }),
        };
        self.permissions.insert(request_id, offered);

        let mut out = self.ensure_turn(line);
        out.push(at(line, Event::PermissionRequested(event)));
        out.push(state(line, LifecycleState::WaitingUser));
        out
    }

    fn control_response(
        &mut self,
        line: &RawLine,
        answer: CliResponse,
        value: Value,
    ) -> Vec<Projected> {
        match answer {
            CliResponse::Success(success) if self.initialize_ids.remove(&success.request_id) => {
                let reply: InitializeResponse = match success.payload() {
                    Ok(reply) => reply,
                    Err(e) => {
                        return error_and_passthrough(
                            line,
                            format!(
                                "raw line {}: unexpected initialize response: {e}",
                                line.index
                            ),
                            value,
                        );
                    }
                };
                // Only the version is projected; the rest (commands, models,
                // account) is for the developer pane until something needs it.
                match reply.claude_code_version.filter(|v| newer_than_tested(v)) {
                    Some(version) => vec![at(
                        line,
                        Event::SessionNotice(SessionNotice {
                            level: NoticeLevel::Warning,
                            code: NoticeCode::BackendVersionUntested,
                            message: format!(
                                "Claude Code {version} is newer than the newest version \
                                 Ersa was tested with ({MAX_TESTED_VERSION})."
                            ),
                        }),
                    )],
                    None => vec![passthrough(line, value)],
                }
            }
            CliResponse::Error(refusal) if self.initialize_ids.remove(&refusal.request_id) => {
                vec![at(
                    line,
                    Event::Error(ErrorEvent {
                        code: ErrorCode::BackendError,
                        message: format!(
                            "Claude Code refused to start the session: {}",
                            refusal.error
                        ),
                        recoverable: false,
                        action: Some(ErrorAction::Restart),
                    }),
                )]
            }
            // Answers to interrupts and other host requests.
            _ => vec![passthrough(line, value)],
        }
    }

    /// Opens a turn when a line belonging to one arrives with none open. A
    /// host user message normally opens it; this covers one-shot runs, whose
    /// prompt was a command-line argument and never a host line.
    fn ensure_turn(&mut self, line: &RawLine) -> Vec<Projected> {
        if self.turn_open {
            Vec::new()
        } else {
            self.open_turn(line, TurnTrigger::System)
        }
    }

    fn open_turn(&mut self, line: &RawLine, trigger: TurnTrigger) -> Vec<Projected> {
        self.turn += 1;
        self.turn_open = true;
        vec![
            at(
                line,
                Event::TurnStarted(TurnStarted {
                    turn: self.turn,
                    trigger,
                }),
            ),
            state(line, LifecycleState::Running),
        ]
    }
}

// ---------------------------------------------------------------------------
// Host lines

impl ClaudeProjector {
    fn host(&mut self, line: &RawLine, value: Value) -> Vec<Projected> {
        match value.get("type").and_then(Value::as_str) {
            Some("user") => self.host_user(line, value),
            Some("control_request") => {
                if value.pointer("/request/subtype").and_then(Value::as_str) == Some("initialize")
                    && let Some(id) = value.get("request_id").and_then(Value::as_str)
                {
                    self.initialize_ids.insert(id.to_string());
                }
                vec![passthrough(line, value)]
            }
            Some("control_response") => self.host_answer(line, value),
            _ => vec![passthrough(line, value)],
        }
    }

    /// A user message the host sent. It starts a turn: the CLI's `system/init`
    /// for the turn only arrives after it.
    fn host_user(&mut self, line: &RawLine, value: Value) -> Vec<Projected> {
        let Some(text) = value.pointer("/message/content").and_then(input_text) else {
            return vec![passthrough(line, value)];
        };
        // A message sent while a turn runs joins that turn rather than
        // starting one the CLI has not started.
        let mut out = if self.turn_open {
            Vec::new()
        } else {
            self.open_turn(line, TurnTrigger::User)
        };
        out.push(at(
            line,
            Event::UserMessage(UserMessage {
                text,
                origin: MessageOrigin::Human,
                peer: None,
            }),
        ));
        out
    }

    /// The host's `control_response`: the user's answer to a permission request.
    fn host_answer(&mut self, line: &RawLine, value: Value) -> Vec<Projected> {
        let request_id = value
            .pointer("/response/request_id")
            .and_then(Value::as_str);
        let Some((request_id, offered)) =
            request_id.and_then(|id| Some((id.to_string(), self.permissions.remove(id)?)))
        else {
            // An answer to something other than a pending permission request.
            return vec![passthrough(line, value)];
        };
        let decision = value.pointer("/response/response");
        let field = |name: &str| decision.and_then(|d| d.get(name));
        let behavior = match field("behavior").and_then(Value::as_str) {
            Some("allow") => PermissionBehavior::Allow,
            _ => PermissionBehavior::Deny,
        };
        // Which offered suggestions the answer applied, as indices into the
        // event's `suggestions`.
        let applied: Vec<u32> = field("updatedPermissions")
            .and_then(Value::as_array)
            .map(|updates| {
                updates
                    .iter()
                    .filter_map(|update| offered.iter().position(|(_, s)| &s.payload == update))
                    .map(|index| index as u32)
                    .collect()
            })
            .unwrap_or_default();
        vec![
            at(
                line,
                Event::PermissionDecided(PermissionDecided {
                    request_id,
                    behavior,
                    decided_by: DecidedBy::User,
                    message: field("message").and_then(Value::as_str).map(String::from),
                    applied_suggestions: (!applied.is_empty()).then_some(applied),
                }),
            ),
            state(line, LifecycleState::Running),
        ]
    }
}

// ---------------------------------------------------------------------------
// Helpers

/// An event that came from `line` alone.
fn at(line: &RawLine, event: Event) -> Projected {
    Projected::from_line(line, event)
}

fn state(line: &RawLine, state: LifecycleState) -> Projected {
    at(
        line,
        Event::SessionState(SessionState {
            state,
            reason: None,
        }),
    )
}

fn block_id(message_id: &str, index: u64) -> String {
    format!("{message_id}/{index}")
}

/// The fallback for a line that maps to nothing else (PROTO-AGENT-EVENTS-R02).
fn passthrough(line: &RawLine, payload: Value) -> Projected {
    at(
        line,
        Event::BackendPassthrough(BackendPassthrough {
            backend: BACKEND.into(),
            kind: kind_of(&payload),
            payload,
        }),
    )
}

/// `type` and the subtype that matters for it, joined with `/`, such as
/// `system/status` or `control_request/initialize`.
fn kind_of(payload: &Value) -> String {
    let Some(kind) = payload.get("type").and_then(Value::as_str) else {
        return "unknown".into();
    };
    let subtype = match kind {
        "control_request" => payload.pointer("/request/subtype"),
        "control_response" => payload.pointer("/response/subtype"),
        "stream_event" => payload.pointer("/event/type"),
        _ => payload.get("subtype"),
    };
    match subtype.and_then(Value::as_str) {
        Some(subtype) => format!("{kind}/{subtype}"),
        None => kind.to_string(),
    }
}

fn protocol_error(line: &RawLine, message: String) -> Projected {
    at(
        line,
        Event::Error(ErrorEvent {
            code: ErrorCode::Protocol,
            message,
            recoverable: true,
            action: None,
        }),
    )
}

/// A line that could not be projected: a recoverable error, then the line
/// itself, and projection carries on (PROTO-AGENT-EVENTS-R10).
fn error_and_passthrough(line: &RawLine, message: String, payload: Value) -> Vec<Projected> {
    vec![protocol_error(line, message), passthrough(line, payload)]
}

fn unparsable(line: &RawLine, message: String) -> Vec<Projected> {
    vec![
        protocol_error(line, message),
        at(
            line,
            Event::BackendPassthrough(BackendPassthrough {
                backend: BACKEND.into(),
                kind: "unparsed".into(),
                payload: Value::String(line.line.clone()),
            }),
        ),
    ]
}

/// The kinds the wire models; a line of one of these that still lands in an
/// `Unknown`/`Other` variant did not fit its model.
const MODELLED_TYPES: &[&str] = &[
    "system",
    "assistant",
    "user",
    "stream_event",
    "result",
    "rate_limit_event",
    "control_request",
    "control_response",
    "control_cancel_request",
    "keep_alive",
];
const MODELLED_SYSTEM: &[&str] = &[
    "init",
    "status",
    "task_summary",
    "post_turn_summary",
    "thinking_tokens",
    "permission_denied",
];

/// The kind of a line the wire models but could not type, if this is one.
fn misparsed(message: &CliMessage) -> Option<String> {
    match message {
        CliMessage::Unknown(value) => {
            let kind = value.get("type")?.as_str()?;
            MODELLED_TYPES.contains(&kind).then(|| kind.to_string())
        }
        CliMessage::System(SystemMessage::Other(value)) => {
            let subtype = value.get("subtype")?.as_str()?;
            MODELLED_SYSTEM
                .contains(&subtype)
                .then(|| format!("system/{subtype}"))
        }
        CliMessage::ControlRequest(request) => match &request.request {
            CliRequest::Other(body) if body.get("subtype")? == "can_use_tool" => {
                Some("control_request/can_use_tool".into())
            }
            _ => None,
        },
        _ => None,
    }
}

/// The text of a message's text blocks, if it has any.
fn plain_text(blocks: &[ContentBlock]) -> Option<String> {
    let texts: Vec<&str> = blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    (!texts.is_empty()).then(|| texts.join("\n"))
}

/// The text of a host user message's `content`: a string, or text blocks.
fn input_text(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => {
            let texts: Vec<&str> = blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect();
            (!texts.is_empty()).then(|| texts.join("\n"))
        }
        _ => None,
    }
}

/// A tool's output: plain text when it is only text, else the blocks as they
/// came (images, say).
fn tool_output(content: Option<MessageContent>) -> ToolResultContent {
    match content {
        None => ToolResultContent::Text(String::new()),
        Some(MessageContent::Text(text)) => ToolResultContent::Text(text),
        Some(MessageContent::Blocks(blocks)) => match plain_text(&blocks) {
            Some(text)
                if blocks
                    .iter()
                    .all(|b| matches!(b, ContentBlock::Text { .. })) =>
            {
                ToolResultContent::Text(text)
            }
            _ => ToolResultContent::Structured(
                serde_json::to_value(&blocks).unwrap_or(Value::Array(Vec::new())),
            ),
        },
    }
}

/// How a turn ended. Only `success` has been recorded; the others follow the
/// SDK's result subtypes until an interrupt or limit is recorded.
fn outcome(result: &ResultMessage) -> TurnOutcome {
    let reason = result.terminal_reason.as_deref().unwrap_or("");
    if result.subtype == "error_max_turns" {
        TurnOutcome::MaxTurns
    } else if reason.contains("interrupt") || reason.contains("abort") {
        TurnOutcome::Interrupted
    } else if result.is_error || result.subtype.starts_with("error") {
        TurnOutcome::Error
    } else {
        TurnOutcome::Success
    }
}

/// The size of the context after the turn, for the context ring.
///
/// `usage`'s top-level counts are summed over every API call in the turn, so
/// they count the shared prefix once per call. The context the next request
/// starts from is what the last call sent and received; `usage.iterations`
/// lists the calls, so its last entry is used when present. Zero means the
/// turn made no call (a failed login, say) and says nothing about the context.
fn context_tokens(usage: &Usage) -> Option<u64> {
    const COUNTS: [&str; 4] = [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
        "output_tokens",
    ];
    let last_call = usage
        .extra
        .get("iterations")
        .and_then(Value::as_array)
        .and_then(|calls| calls.last());
    let total = match last_call {
        Some(call) => COUNTS
            .iter()
            .filter_map(|count| call.get(*count).and_then(Value::as_u64))
            .sum(),
        None => [
            usage.input_tokens,
            usage.cache_creation_input_tokens,
            usage.cache_read_input_tokens,
            usage.output_tokens,
        ]
        .into_iter()
        .flatten()
        .sum(),
    };
    (total > 0).then_some(total)
}

fn rate_limit(info: &RateLimitInfo) -> RateLimit {
    RateLimit {
        status: info.status.clone(),
        resets_at: info.resets_at.map(rfc3339_from_unix),
        // Which limit this is, such as `seven_day`.
        detail: info.rate_limit_type.clone(),
    }
}

/// Whether `version` is newer than [`MAX_TESTED_VERSION`]. A version that does
/// not read as dotted numbers is not compared, and gets no warning.
fn newer_than_tested(version: &str) -> bool {
    match (version_parts(version), version_parts(MAX_TESTED_VERSION)) {
        (Some(version), Some(tested)) => version > tested,
        _ => false,
    }
}

/// `2.1.302` as `[2, 1, 302]`; a suffix such as `-beta` on a part is ignored.
fn version_parts(version: &str) -> Option<Vec<u64>> {
    version
        .split('.')
        .map(|part| {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .collect()
}

/// Removes ANSI escape sequences (`ESC [ … letter`), which the CLI puts in
/// some reasons meant for its terminal UI.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            // Parameters and intermediates, up to the final letter-like byte.
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Unix seconds as an RFC 3339 UTC time, such as `2026-11-04T20:00:00Z`.
fn rfc3339_from_unix(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}

/// The calendar date `days` after 1970-01-01, in the proleptic Gregorian
/// calendar. Howard Hinnant's `civil_from_days`: shift to a calendar whose
/// year starts in March (so the leap day is last), then count 400-year eras.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153; // 0 is March
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_unix_seconds_to_rfc3339() {
        assert_eq!(rfc3339_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_from_unix(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(rfc3339_from_unix(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn compares_versions_numerically() {
        assert!(!newer_than_tested("2.1.302"));
        assert!(!newer_than_tested("2.1.99"));
        assert!(newer_than_tested("2.1.303"));
        assert!(newer_than_tested("2.10.0"));
        assert!(newer_than_tested("3.0.0-beta"));
        assert!(!newer_than_tested("nightly"));
    }

    #[test]
    fn strips_ansi_escapes() {
        assert_eq!(
            strip_ansi("\u{1b}[1mBash\u{1b}[22m needs approval"),
            "Bash needs approval"
        );
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn kinds_join_type_and_subtype() {
        let kind = |v: Value| kind_of(&v);
        assert_eq!(
            kind(serde_json::json!({"type":"system","subtype":"status"})),
            "system/status"
        );
        assert_eq!(
            kind(serde_json::json!({"type":"control_request","request":{"subtype":"initialize"}})),
            "control_request/initialize"
        );
        assert_eq!(kind(serde_json::json!({"type":"keep_alive"})), "keep_alive");
        assert_eq!(kind(serde_json::json!([1])), "unknown");
    }
}
