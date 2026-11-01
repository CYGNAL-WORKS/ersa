//! What the CLI writes to stdout: one JSON message per line.

use std::collections::{BTreeMap, BTreeSet};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::content::{ApiMessage, Extra, MessageContent, StreamEvent, Usage};
use super::permissions::PermissionUpdate;

/// One stdout line, picked by its `type` field.
///
/// A line whose `type` is not modelled parses into [`CliMessage::Unknown`] with
/// the whole line as raw JSON, so a newer CLI never makes a line unparseable.
/// The only error is a line that is not valid JSON.
///
/// Serde tries the untagged `Unknown` variant whenever the tagged ones fail, so
/// a line whose `type` is modelled but whose fields do not fit (a required field
/// missing, say) also lands in `Unknown` rather than being dropped. That means
/// the model is wrong; the fixture conformance test fails on it, and a driver
/// seeing `Unknown` with a modelled `type` should warn. The same holds for the
/// other `Other` variants in this module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CliMessage {
    System(SystemMessage),
    Assistant(AssistantMessage),
    User(UserMessage),
    StreamEvent(StreamEventMessage),
    Result(ResultMessage),
    RateLimitEvent(RateLimitEvent),
    ControlRequest(CliControlRequest),
    ControlResponse(CliControlResponse),
    ControlCancelRequest(ControlCancelRequest),
    /// A liveness heartbeat, sent for example while a slow control request is
    /// in progress. It carries no payload and is to be ignored. Unrecorded;
    /// modelled from the SDK types.
    KeepAlive {
        #[serde(flatten)]
        extra: Extra,
    },
    #[serde(untagged)]
    Unknown(Value),
}

impl CliMessage {
    /// Parses one stdout line.
    pub fn from_line(line: &str) -> serde_json::Result<Self> {
        serde_json::from_str(line)
    }
}

/// A `system` line, picked by its `subtype` field. Unmodelled subtypes land in
/// `Other`, which holds the line without its `type` key (always `system`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum SystemMessage {
    /// Boxed because it is far larger than the other variants, and an enum is as
    /// large as its largest variant.
    Init(Box<SystemInit>),
    Status(SystemStatus),
    TaskSummary(TaskSummary),
    PostTurnSummary(PostTurnSummary),
    ThinkingTokens(ThinkingTokens),
    PermissionDenied(PermissionDenied),
    #[serde(untagged)]
    Other(Value),
}

/// `system/init`: the first line of every session, repeated on resume and at the
/// start of each turn in a bidirectional session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemInit {
    pub session_id: String,
    pub uuid: String,
    pub claude_code_version: String,
    /// Protocol features this CLI supports. An open set: unknown values are
    /// expected and ignored.
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub model: String,
    #[serde(rename = "permissionMode")]
    pub permission_mode: String,
    #[serde(rename = "apiKeySource")]
    pub api_key_source: String,
    pub cwd: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerStatus>,
    #[serde(default)]
    pub slash_commands: Vec<String>,
    #[serde(default)]
    pub output_style: Option<String>,
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub plugins: Vec<PluginInfo>,
    #[serde(default)]
    pub fast_mode_state: Option<String>,
    #[serde(default)]
    pub view_mode: Option<String>,
    #[serde(default)]
    pub powershell_path: Option<String>,
    #[serde(default)]
    pub startup_timing: Option<Value>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl SystemInit {
    /// What feature detection reads. PROTO-CLAUDE-CODE-STREAM says behaviour is
    /// decided from these and never from version strings elsewhere.
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            claude_code_version: self.claude_code_version.clone(),
            flags: self.capabilities.iter().cloned().collect(),
        }
    }
}

/// The CLI's version and capability strings, as `system/init` reports them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub claude_code_version: String,
    pub flags: BTreeSet<String>,
}

impl Capabilities {
    /// Whether the CLI advertises `name`, such as `interrupt_receipt_v1`.
    pub fn has(&self, name: &str) -> bool {
        self.flags.contains(name)
    }
}

/// One MCP server as `system/init` lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One plugin as `system/init` lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `system/status`: what the CLI is doing, such as `requesting`. `null` when idle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemStatus {
    pub session_id: String,
    pub uuid: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, rename = "permissionMode")]
    pub permission_mode: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `system/task_summary`: a one-line description of the work in progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskSummary {
    pub session_id: String,
    pub uuid: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `system/post_turn_summary`: how a turn ended, for the rail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostTurnSummary {
    pub session_id: String,
    pub uuid: String,
    /// `completed` or `blocked` in the fixtures.
    pub status_category: String,
    #[serde(default)]
    pub status_detail: Option<String>,
    #[serde(default)]
    pub needs_action: Option<String>,
    #[serde(default)]
    pub summarizes_uuid: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `system/thinking_tokens`: running estimate of reasoning tokens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThinkingTokens {
    pub session_id: String,
    pub uuid: String,
    pub estimated_tokens: i64,
    pub estimated_tokens_delta: i64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `system/permission_denied`: a tool needed approval and nobody could answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionDenied {
    pub session_id: String,
    pub uuid: String,
    pub tool_name: String,
    pub tool_use_id: String,
    pub message: String,
    #[serde(default)]
    pub decision_reason_type: Option<String>,
    #[serde(default)]
    pub decision_reason: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// An `assistant` line: in practice one content block per line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub session_id: String,
    pub uuid: String,
    pub message: ApiMessage,
    /// `None` on the main thread; the spawning tool call's id inside a subagent.
    #[serde(default)]
    pub parent_tool_use_id: Option<String>,
    /// Set on synthetic messages, such as `authentication_failed`.
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl AssistantMessage {
    /// The CLI's synthetic "Not logged in" message. It belongs in a session error,
    /// not the transcript.
    pub fn is_authentication_failure(&self) -> bool {
        self.error.as_deref() == Some("authentication_failed")
    }
}

/// A `user` line: a replayed stdin message or the result of a tool the CLI ran.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserMessage {
    pub session_id: String,
    pub uuid: String,
    pub message: UserApiMessage,
    #[serde(default)]
    pub parent_tool_use_id: Option<String>,
    /// True when the CLI echoes a stdin message (`--replay-user-messages`).
    #[serde(default, rename = "isReplay")]
    pub is_replay: Option<bool>,
    /// The tool's structured output; its shape depends on the tool.
    #[serde(default)]
    pub tool_use_result: Option<Value>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// The Messages API user message inside a `user` line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserApiMessage {
    pub role: String,
    pub content: MessageContent,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A `stream_event` line (only with `--include-partial-messages`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamEventMessage {
    pub session_id: String,
    pub uuid: String,
    pub event: StreamEvent,
    #[serde(default)]
    pub parent_tool_use_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A `result` line: the end of a turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultMessage {
    // Fields nothing depends on are defaulted, so a newer CLI dropping one
    // does not make the whole line untypable.
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub uuid: String,
    /// `success` or one of the SDK's `error_*` subtypes. Kept as a string so a
    /// new subtype still parses; `is_error` is the field to branch on.
    pub subtype: String,
    pub is_error: bool,
    #[serde(default)]
    pub num_turns: u64,
    pub duration_ms: u64,
    #[serde(default)]
    pub duration_api_ms: u64,
    /// The cost tile's source.
    pub total_cost_usd: f64,
    /// The context ring's source.
    pub usage: Usage,
    #[serde(default, rename = "modelUsage")]
    pub model_usage: BTreeMap<String, ModelUsage>,
    #[serde(default)]
    pub permission_denials: Vec<PermissionDenial>,
    /// Why the turn ended, such as `completed` or `api_error`.
    #[serde(default)]
    pub terminal_reason: Option<String>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    /// The final text, on success.
    #[serde(default)]
    pub result: Option<String>,
    /// Error descriptions, on the `error_*` subtypes.
    #[serde(default)]
    pub errors: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Per-model totals on `result`, keyed by model id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default, rename = "costUSD")]
    pub cost_usd: Option<f64>,
    /// The model's context window in tokens: the context ring's denominator.
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A tool call that was denied during the turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionDenial {
    pub tool_name: String,
    pub tool_use_id: String,
    pub tool_input: Value,
}

/// A `rate_limit_event` line: the login's rate-limit status, for the budget readout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RateLimitEvent {
    pub session_id: String,
    pub uuid: String,
    pub rate_limit_info: RateLimitInfo,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitInfo {
    /// `allowed`, `allowed_warning`, or `rejected`.
    pub status: String,
    /// Unix seconds.
    #[serde(default)]
    pub resets_at: Option<i64>,
    #[serde(default)]
    pub rate_limit_type: Option<String>,
    #[serde(default)]
    pub utilization: Option<f64>,
    #[serde(default)]
    pub overage_status: Option<String>,
    #[serde(default)]
    pub is_using_overage: Option<bool>,
    /// Utilisation per window, keyed by window name such as `seven_day`.
    #[serde(default)]
    pub unified_windows: Option<BTreeMap<String, RateLimitWindow>>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitWindow {
    /// Fraction of the window used, 0 to 1.
    pub utilization: f64,
    #[serde(default)]
    pub resets_at: Option<i64>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A `control_request` the CLI sends to the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CliControlRequest {
    /// Chosen by the CLI; the host's answer must echo it.
    pub request_id: String,
    pub request: CliRequest,
}

/// The body of a CLI-to-host control request, picked by `subtype`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum CliRequest {
    /// Boxed for the same reason as [`SystemMessage::Init`].
    CanUseTool(Box<CanUseTool>),
    #[serde(untagged)]
    Other(Value),
}

/// A permission prompt: the CLI asks whether a tool call may run. The shelf
/// renders `display_name`, `description`, `input`, and `blocked_path`, and offers
/// `permission_suggestions` as "allow for this session".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanUseTool {
    pub tool_name: String,
    pub tool_use_id: String,
    pub input: Value,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub blocked_path: Option<String>,
    /// `null` reads as none.
    #[serde(default, deserialize_with = "null_as_default")]
    pub permission_suggestions: Vec<PermissionUpdate>,
    /// Why approval is needed, for the consent line. Usually a string, which
    /// may contain ANSI escapes; kept as raw JSON so another shape still parses.
    #[serde(default)]
    pub decision_reason: Option<Value>,
    #[serde(default)]
    pub decision_reason_type: Option<String>,
    #[serde(default)]
    pub classifier_approvable: Option<bool>,
    /// When true, offer no "always allow" option.
    #[serde(default)]
    pub suppress_always_allow_rule: Option<bool>,
    /// When true, do not pre-select approve.
    #[serde(default)]
    pub default_to_no: Option<bool>,
    #[serde(default)]
    pub requires_user_interaction: Option<bool>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Reads `null` as the type's default (an empty list, say), for fields the
/// CLI sometimes sends as `null` rather than leaving out.
fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// The sender withdraws one of its own in-flight control requests and ignores
/// any answer that still arrives. Either side may send it. From the CLI it
/// typically withdraws a pending `can_use_tool` prompt, for example after an
/// interrupt, so the shelf should drop that prompt. Unrecorded; modelled from
/// the SDK types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlCancelRequest {
    pub request_id: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A `control_response` the CLI sends: its answer to one of the host's requests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CliControlResponse {
    pub response: CliResponse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum CliResponse {
    Success(ControlSuccess),
    Error(ControlError),
    #[serde(untagged)]
    Other(Value),
}

/// A successful answer. The payload's shape depends on which request it
/// answers, which only the host knows (by `request_id`), so it stays raw here
/// and [`ControlSuccess::payload`] reads it as the expected type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlSuccess {
    pub request_id: String,
    #[serde(default)]
    pub response: Option<Value>,
    /// `can_use_tool` requests still unanswered, so a host joining a running
    /// session learns about them.
    #[serde(default)]
    pub pending_permission_requests: Vec<CliControlRequest>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl ControlSuccess {
    /// Reads the payload as `T`, such as [`InitializeResponse`]. A missing
    /// payload reads as an empty object, which the SDK types allow for requests
    /// that are merely acknowledged.
    pub fn payload<T: DeserializeOwned>(&self) -> serde_json::Result<T> {
        let value = self
            .response
            .clone()
            .unwrap_or_else(|| Value::Object(Default::default()));
        serde_json::from_value(value)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlError {
    pub request_id: String,
    pub error: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// The payload answering `initialize`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitializeResponse {
    #[serde(default)]
    pub claude_code_version: Option<String>,
    #[serde(default)]
    pub commands: Vec<SlashCommand>,
    #[serde(default)]
    pub agents: Vec<AgentInfo>,
    #[serde(default)]
    pub models: Vec<ModelInfo>,
    #[serde(default)]
    pub account: Option<AccountInfo>,
    #[serde(default)]
    pub output_style: Option<String>,
    #[serde(default)]
    pub current_permission_mode: Option<String>,
    /// Observed in 2.1.302 but absent from the SDK types, and a different list
    /// from `system/init`'s. Feature detection reads `system/init`.
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommand {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub argument_hint: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// What `set_model` takes, such as `default`.
    pub value: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default)]
    pub subscription_type: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}
