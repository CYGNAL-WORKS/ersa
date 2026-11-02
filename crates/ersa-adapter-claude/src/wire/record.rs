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

use super::content::{ApiMessage, ContentBlock, Extra, MessageContent};
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
    /// Something the CLI noted itself; only `local_command` is read.
    System(RecordSystem),
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

/// How the CLI marks up text it writes into the conversation itself: a slash
/// command and its output, a shell command typed with `!`, a background task's
/// report, an interrupt. Its own fixed markers, never translated.
const CLI_MARKUP: &[&str] = &[
    "<command-name>",
    "<command-message>",
    "<command-args>",
    "<local-command-",
    "<bash-input>",
    "<bash-stdout>",
    "<bash-stderr>",
    "<task-notification>",
    "[Request interrupted by user",
];

impl RecordUser {
    /// Whether the CLI wrote this message rather than a person typing a prompt
    /// (or a tool returning a result): flagged as meta or as a compaction
    /// summary, from an origin other than a person, or in the CLI's markup.
    pub fn is_from_cli(&self) -> bool {
        if self.is_meta == Some(true) || self.is_compact_summary == Some(true) {
            return true;
        }
        let origin = self
            .extra
            .get("origin")
            .and_then(|origin| origin.get("kind"))
            .and_then(Value::as_str);
        if origin.is_some_and(|kind| kind != "human") {
            return true;
        }
        let text = match &self.message.content {
            MessageContent::Text(text) => Some(text.as_str()),
            MessageContent::Blocks(blocks) => blocks.iter().find_map(|block| match block {
                ContentBlock::Text { text, .. } => Some(text.as_str()),
                _ => None,
            }),
        };
        text.is_some_and(|text| {
            let text = text.trim_start();
            CLI_MARKUP.iter().any(|mark| text.starts_with(mark))
        })
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

/// A `system` entry. With subtype `local_command`, `content` is a local
/// command's output (from `/rename`, say) wrapped in
/// `<local-command-stdout>`, which the stream carried as a synthetic
/// `assistant` message with the same `uuid`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordSystem {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl RecordSystem {
    /// The text a local command printed, as the stream's synthetic message
    /// carries it: the content without its output tag. `None` for any other
    /// subtype, and for content that is not a command's output (the echo of
    /// the command itself, which older versions also wrote here).
    pub fn local_command_output(&self) -> Option<&str> {
        if self.subtype.as_deref() != Some("local_command") {
            return None;
        }
        let content = self.content.as_deref()?.trim();
        ["stdout", "stderr"].into_iter().find_map(|stream| {
            content
                .strip_prefix(&format!("<local-command-{stream}>"))?
                .strip_suffix(&format!("</local-command-{stream}>"))
                .map(str::trim)
        })
    }
}
