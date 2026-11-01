//! What the CLI writes to its session record,
//! `~/.claude/projects/<encoded cwd>/<session id>.jsonl`: one JSON entry per
//! line (PROTO-CLAUDE-CODE-STREAM, Session record).
//!
//! Only the entries the adapter reads are modelled. Their `message` is the same
//! Messages API message the live stream carries, so it reuses the stream's
//! types; the wrapper around it is internal to the CLI and changes between
//! versions, so every wrapper field is optional or kept in `extra`. Every
//! other kind of entry (attachments, snapshots, queue operations and the like)
//! parses into [`RecordEntry::Other`] and is ignored.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::content::{ApiMessage, Extra};
use super::from_cli::UserApiMessage;

/// One line of a session record, picked by its `type` field.
///
/// As with [`super::CliMessage`], an entry whose `type` is modelled but whose
/// fields do not fit lands in `Other` too; the record conformance test fails
/// on that, and the projection reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RecordEntry {
    User(RecordUser),
    Assistant(RecordAssistant),
    /// A title the user gave the session.
    CustomTitle(CustomTitle),
    /// A title the CLI generated for the session.
    AiTitle(AiTitle),
    /// The latest prompt, which the CLI appends after each one.
    LastPrompt(LastPrompt),
    #[serde(untagged)]
    Other(Value),
}

impl RecordEntry {
    /// Parses one record line.
    pub fn from_line(line: &str) -> serde_json::Result<Self> {
        serde_json::from_str(line)
    }
}

/// A `user` entry: a prompt, or the result of a tool the CLI ran.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordUser {
    /// The id the live stream gives the same message; record and live input
    /// merge on it.
    pub uuid: String,
    pub message: UserApiMessage,
    #[serde(default, rename = "sessionId")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
    /// Set on text the CLI added to the conversation itself (expanded slash
    /// commands, caveats), not typed by anyone.
    #[serde(default, rename = "isMeta")]
    pub is_meta: Option<bool>,
    /// Set on the summary that replaces the conversation after a compaction.
    #[serde(default, rename = "isCompactSummary")]
    pub is_compact_summary: Option<bool>,
    /// On a tool result: how the call was permitted.
    #[serde(default, rename = "permissionDecision")]
    pub permission_decision: Option<RecordPermissionDecision>,
    /// The tool's structured output, as the stream's `tool_use_result`.
    #[serde(default, rename = "toolUseResult")]
    pub tool_use_result: Option<Value>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl RecordUser {
    /// Whether the CLI wrote this message rather than a person or a tool.
    pub fn is_from_cli(&self) -> bool {
        self.is_meta == Some(true) || self.is_compact_summary == Some(true)
    }
}

/// The permission decision the record keeps on a tool result. Observed on
/// 2.1.316: `decision` `accept`, with `source` `config` (and `reasonType` such
/// as `mode`, `rule`, `classifier`) or `user_temporary` (and `grantScope`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordPermissionDecision {
    pub decision: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default, rename = "reasonType")]
    pub reason_type: Option<String>,
    #[serde(default, rename = "grantScope")]
    pub grant_scope: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// An `assistant` entry: one content block of a reply, as on the stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordAssistant {
    pub uuid: String,
    pub message: ApiMessage,
    #[serde(default, rename = "sessionId")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
    /// Set on synthetic messages, such as `authentication_failed`, as on the
    /// stream.
    #[serde(default)]
    pub error: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomTitle {
    #[serde(rename = "customTitle")]
    pub custom_title: String,
    #[serde(default, rename = "sessionId")]
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiTitle {
    #[serde(rename = "aiTitle")]
    pub ai_title: String,
    #[serde(default, rename = "sessionId")]
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LastPrompt {
    /// Absent on some entries in the wild.
    #[serde(default, rename = "lastPrompt")]
    pub last_prompt: Option<String>,
    #[serde(default, rename = "sessionId")]
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}
