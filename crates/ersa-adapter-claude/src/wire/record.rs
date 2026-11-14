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
    /// On a prompt: the permission mode its turn started with. Tool results,
    /// meta and local-command entries carry none.
    #[serde(default, rename = "permissionMode")]
    pub permission_mode: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// How a slash command's echo begins.
const COMMAND_MARKUP: &[&str] = &["<command-name>", "<command-message>", "<command-args>"];

/// How the CLI marks up text it writes into the conversation itself: a slash
/// command and its output, a shell command typed with `!`, a background task's
/// report, an interrupt. Its own fixed markers, never translated.
const CLI_MARKUP: &[&str] = &[
    "<local-command-",
    "<bash-input>",
    "<bash-stdout>",
    "<bash-stderr>",
    TASK_REPORT_MARKER,
    INTERRUPT_MARKER,
];

/// How a background task's report begins.
const TASK_REPORT_MARKER: &str = "<task-notification>";

/// How the CLI's note of an interrupted turn begins; the rest varies
/// (`[Request interrupted by user for tool use]`).
const INTERRUPT_MARKER: &str = "[Request interrupted by user";

impl RecordUser {
    /// Whether the CLI wrote this message rather than a person typing a prompt
    /// (or a tool returning a result): flagged as meta or as a compaction
    /// summary, from an origin other than a person, or in the CLI's markup.
    pub fn is_from_cli(&self) -> bool {
        if self.is_meta == Some(true) || self.is_compact_summary == Some(true) {
            return true;
        }
        if self.has_non_human_origin() {
            return true;
        }
        self.first_text().is_some_and(|text| {
            COMMAND_MARKUP
                .iter()
                .chain(CLI_MARKUP)
                .any(|mark| text.starts_with(mark))
        })
    }

    fn has_non_human_origin(&self) -> bool {
        self.extra
            .get("origin")
            .and_then(|origin| origin.get("kind"))
            .and_then(Value::as_str)
            .is_some_and(|kind| kind != "human")
    }

    /// The first text of the message, as the CLI's markup is matched.
    fn first_text(&self) -> Option<&str> {
        match &self.message.content {
            MessageContent::Text(text) => Some(text.as_str()),
            MessageContent::Blocks(blocks) => blocks.iter().find_map(|block| match block {
                ContentBlock::Text { text, .. } => Some(text.as_str()),
                _ => None,
            }),
        }
        .map(str::trim_start)
    }

    /// Whether this is the CLI's marker that the user interrupted the turn.
    pub fn is_interrupt_marker(&self) -> bool {
        self.first_text()
            .is_some_and(|text| text.starts_with(INTERRUPT_MARKER))
    }

    /// Whether this is the echo of a slash command the user ran. A local
    /// command's echo has its output after it; any other command, a skill
    /// say, goes on to a reply (fixture 2.1.316/03-local-command).
    pub fn is_command_echo(&self) -> bool {
        self.first_text()
            .is_some_and(|text| COMMAND_MARKUP.iter().any(|mark| text.starts_with(mark)))
    }

    /// Whether the CLI wrote this beside the conversation, with no reply to
    /// follow: a local command's caveat or output, a compaction summary. A
    /// background task's report starts a turn, so it is not one, and neither
    /// is the interrupt marker or a command's echo, which the caller judges
    /// by what follows it.
    pub fn is_aside(&self) -> bool {
        let starts_a_turn = self.has_non_human_origin()
            || self
                .first_text()
                .is_some_and(|text| text.starts_with(TASK_REPORT_MARKER));
        self.is_from_cli()
            && !starts_a_turn
            && !self.is_interrupt_marker()
            && !self.is_command_echo()
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
    /// The effort the reply was made with, such as `medium`.
    #[serde(default)]
    pub effort: Option<String>,
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
/// command's output (from `/rename`, say) inside `<local-command-stdout>` or
/// `<local-command-stderr>` tags, which the stream carried as a synthetic
/// `assistant` message with the same `uuid`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordSystem {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub subtype: Option<String>,
    /// A string on every entry seen; kept as JSON so that one that is not
    /// still parses, and is reported as drifted rather than dropped.
    #[serde(default)]
    pub content: Option<Value>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// What a `system` entry's content says about a local command.
#[derive(Debug, Clone, PartialEq)]
pub enum LocalCommandOutput<'a> {
    /// Not a `local_command` entry.
    NotLocalCommand,
    /// No output tag (or no content): the command's echo, which older CLIs
    /// wrote beside the output. It shows nothing.
    Echo,
    /// The text inside each output tag, in order, as the stream carried it.
    Output(Vec<&'a str>),
    /// A tag that is not closed, or content that is not a string.
    Drifted,
}

impl RecordSystem {
    /// Reads a local command's output from `content`.
    pub fn local_command_output(&self) -> LocalCommandOutput<'_> {
        if self.subtype.as_deref() != Some("local_command") {
            return LocalCommandOutput::NotLocalCommand;
        }
        // No content at all carries no output to show; it is how the
        // recorded fixtures trim an entry they do not keep.
        let content = match &self.content {
            None => return LocalCommandOutput::Echo,
            Some(Value::String(content)) => content,
            Some(_) => return LocalCommandOutput::Drifted,
        };
        if !content.contains("<local-command-") {
            return LocalCommandOutput::Echo;
        }
        // One wrapper after another; only whitespace may sit between them.
        // The text inside a wrapper is kept exactly.
        let mut texts = Vec::new();
        let mut rest = content.trim();
        while !rest.is_empty() {
            let inner = ["stdout", "stderr"].into_iter().find_map(|stream| {
                let open = format!("<local-command-{stream}>");
                let close = format!("</local-command-{stream}>");
                let after = rest.strip_prefix(open.as_str())?;
                let end = after.find(close.as_str())?;
                Some((&after[..end], &after[end + close.len()..]))
            });
            let Some((text, after)) = inner else {
                return LocalCommandOutput::Drifted;
            };
            texts.push(text);
            rest = after.trim_start();
        }
        LocalCommandOutput::Output(texts)
    }
}
