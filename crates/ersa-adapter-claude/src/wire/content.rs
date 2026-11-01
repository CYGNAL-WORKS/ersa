//! Shapes borrowed from the Anthropic Messages API: content blocks, the message
//! envelope inside `assistant` lines, token usage, and the stream events that
//! `stream_event` lines wrap.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Fields a struct does not model, kept so nothing the CLI sent is lost. Serde's
/// `flatten` puts every key the struct does not name into this map.
pub type Extra = Map<String, Value>;

/// One block of a message's `content` array.
///
/// `#[serde(tag = "type")]` picks the variant from the block's `type` field. The
/// last variant is `untagged`: a block whose `type` is not one of the named ones
/// lands there whole, as raw JSON, instead of failing the line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
        #[serde(flatten)]
        extra: Extra,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
        /// Who issued the call (for example the main thread or a subagent).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caller: Option<Value>,
        #[serde(flatten)]
        extra: Extra,
    },
    /// The model's reasoning. `thinking` is often empty because the CLI withholds
    /// the text; `signature` is still present.
    Thinking {
        thinking: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
        #[serde(flatten)]
        extra: Extra,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<MessageContent>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        #[serde(flatten)]
        extra: Extra,
    },
    #[serde(untagged)]
    Other(Value),
}

/// Message or tool-result content: the Messages API allows either a plain string
/// or an array of blocks. `untagged` tries each variant in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// The Messages API message carried by an `assistant` line and by the
/// `message_start` stream event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiMessage {
    pub id: String,
    pub model: String,
    pub role: String,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Token counts. On `result` this is the turn's total and feeds the context ring.
/// `message_delta` carries a partial copy, so every count is optional.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    pub service_tier: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A Messages API stream event, as wrapped by a `stream_event` line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart {
        message: ApiMessage,
    },
    ContentBlockStart {
        index: u64,
        content_block: ContentBlock,
    },
    ContentBlockDelta {
        index: u64,
        delta: Delta,
    },
    ContentBlockStop {
        index: u64,
    },
    MessageDelta {
        delta: Value,
        #[serde(default)]
        usage: Option<Usage>,
        #[serde(flatten)]
        extra: Extra,
    },
    MessageStop,
    #[serde(untagged)]
    Other(Value),
}

/// An increment to one content block: the standard Messages API deltas. Each
/// keeps fields it does not name, such as the `estimated_tokens` 2.1.316 adds
/// to `thinking_delta`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Delta {
    TextDelta {
        text: String,
        #[serde(flatten)]
        extra: Extra,
    },
    InputJsonDelta {
        partial_json: String,
        #[serde(flatten)]
        extra: Extra,
    },
    ThinkingDelta {
        thinking: String,
        #[serde(flatten)]
        extra: Extra,
    },
    SignatureDelta {
        signature: String,
        #[serde(flatten)]
        extra: Extra,
    },
    #[serde(untagged)]
    Other(Value),
}
