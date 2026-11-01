//! Permission shapes shared by both directions: the suggestions a `can_use_tool`
//! request offers, the decision the host answers with, and permission modes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A change to permission rules or directories. The CLI offers these as
/// `permission_suggestions`; the host echoes the ones the user accepts back in
/// `updatedPermissions`. Field order matches the wire so serialised output reads
/// the same as the CLI's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PermissionUpdate {
    AddRules {
        rules: Vec<PermissionRule>,
        behavior: String,
        destination: String,
    },
    ReplaceRules {
        rules: Vec<PermissionRule>,
        behavior: String,
        destination: String,
    },
    RemoveRules {
        rules: Vec<PermissionRule>,
        behavior: String,
        destination: String,
    },
    SetMode {
        mode: String,
        destination: String,
    },
    AddDirectories {
        directories: Vec<String>,
        destination: String,
    },
    RemoveDirectories {
        directories: Vec<String>,
        destination: String,
    },
    #[serde(untagged)]
    Other(Value),
}

/// One rule: a tool name and, optionally, a pattern such as `echo probe *`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRule {
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_content: Option<String>,
}

/// The host's answer to a `can_use_tool` request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "behavior", rename_all = "lowercase")]
pub enum PermissionDecision {
    #[serde(rename_all = "camelCase")]
    Allow {
        /// The tool input to run with: the original, or the user's edit of it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updated_input: Option<Value>,
        /// Suggestions the user accepted ("allow for this session").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updated_permissions: Option<Vec<PermissionUpdate>>,
    },
    Deny {
        /// Shown to the model as the reason the tool did not run.
        message: String,
        /// True stops the turn as well as the tool.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        interrupt: Option<bool>,
    },
}

/// How the CLI handles tool calls that need approval. Used when the host sets
/// the mode; inbound lines keep the mode as a string so a new mode name does not
/// break parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    Default,
    AcceptEdits,
    BypassPermissions,
    Plan,
    DontAsk,
    Auto,
}
