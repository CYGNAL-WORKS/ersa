//! What the host writes to the CLI's stdin: one JSON message per line, flushed
//! after each (PROTO-CLAUDE-CODE-STREAM-R07, the driver's job).

use serde::Serialize;
use serde_json::{Map, Value};

use super::permissions::{PermissionDecision, PermissionMode};

/// One stdin line. Only serialised, never parsed: the host writes these.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    User(UserInput),
    ControlRequest(HostControlRequest),
    ControlResponse(HostControlResponse),
    /// Withdraws one of the host's own in-flight control requests; the CLI
    /// stops working on it and the host ignores any late answer.
    ControlCancelRequest {
        request_id: String,
    },
}

impl HostMessage {
    /// A user turn holding one text block.
    pub fn user_text(text: impl Into<String>) -> Self {
        HostMessage::User(UserInput {
            message: UserInputMessage {
                role: "user".to_string(),
                content: vec![InputBlock::Text { text: text.into() }],
            },
            parent_tool_use_id: None,
        })
    }

    /// A control request with a host-chosen id; the CLI's answer echoes it.
    pub fn control_request(request_id: impl Into<String>, request: HostRequest) -> Self {
        HostMessage::ControlRequest(HostControlRequest {
            request_id: request_id.into(),
            request,
        })
    }

    /// The answer to a `can_use_tool` request, echoing its `request_id`.
    pub fn permission_decision(
        request_id: impl Into<String>,
        decision: &PermissionDecision,
    ) -> serde_json::Result<Self> {
        Ok(HostMessage::ControlResponse(HostControlResponse {
            response: HostResponse::Success {
                request_id: request_id.into(),
                response: serde_json::to_value(decision)?,
            },
        }))
    }

    /// The JSON line to write, without the trailing newline.
    pub fn to_line(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

/// A user message. The recorded sessions send no `parent_tool_use_id`, although
/// the SDK types mark it required; the CLI accepts its absence.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserInput {
    pub message: UserInputMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserInputMessage {
    pub role: String,
    pub content: Vec<InputBlock>,
}

/// A content block the host sends.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputBlock {
    Text { text: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HostControlRequest {
    pub request_id: String,
    pub request: HostRequest,
}

/// The control requests the adapter sends (ADR 0014). Only `initialize` is
/// recorded; the others follow the SDK types until fixtures exist.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum HostRequest {
    // Boxed: far larger than every other request.
    Initialize(Box<InitializeRequest>),
    Interrupt {
        /// Also cancel queued user messages. Honoured only when `system/init`
        /// advertises `interrupt_cancel_queued_v1`.
        #[serde(skip_serializing_if = "Option::is_none")]
        cancel_queued: Option<bool>,
    },
    SetPermissionMode {
        mode: PermissionMode,
    },
    SetModel {
        /// `None` resets to the session's default model.
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    GetContextUsage {
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<ContextUsageDetail>,
    },
    GetSessionCost,
    /// A JSON-RPC message for an in-process MCP server declared in
    /// `initialize`'s `sdkMcpServers`.
    McpMessage {
        server_name: String,
        message: Value,
    },
}

/// `full` counts each category with the token-count API; `summary` estimates
/// from the last response. The CLI defaults to `full`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextUsageDetail {
    Summary,
    Full,
}

/// The `initialize` request. `hooks` is always sent, as `{}` when empty, to
/// match what the recorded sessions sent.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequest {
    pub hooks: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdk_mcp_servers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdk_mcp_server_configs: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub append_system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_dialog_kinds: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_task_stop_affordance: Option<bool>,
    /// That the user trusts `directory`, so the CLI records the workspace as
    /// trusted in its own configuration (ADR 0018). Sent only for a trusted
    /// project's session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_trust: Option<WorkspaceTrust>,
}

/// The host's attestation that the user accepted a trust question for
/// `directory`. The CLI honours it only when `directory` is an absolute path
/// with the same trust key as the session's working directory (its git root,
/// shared by the repository and its linked worktrees), and answers with
/// `workspace_trust_recorded` in the `initialize` response.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkspaceTrust {
    /// Only ever true: the CLI asks that the host sends this only after
    /// showing the user a trust question for `directory`.
    pub accepted: bool,
    pub directory: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HostControlResponse {
    pub response: HostResponse,
}

/// The host's answer to a CLI control request.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum HostResponse {
    Success { request_id: String, response: Value },
    Error { request_id: String, error: String },
}
