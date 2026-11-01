//! Turning a `can_use_tool` request's `permission_suggestions` into the
//! `suggestions` of a `permission.requested` event.
//!
//! The projector and the driver both need this, and must agree exactly: the
//! user picks suggestions by their index in the event, and the driver turns
//! those indices back into the CLI's own suggestion objects. Keeping the one
//! mapping here is what makes the two agree.
//!
//! Suggestions are read as raw JSON and passed on untouched: the typed model
//! only decides what kind each one is, so fields a newer CLI adds survive the
//! trip to the shelf and back into `updatedPermissions`.

use ersa_protocol::events::{PermissionSuggestion, SuggestionKind};
use serde_json::Value;

use crate::wire::{PermissionRule, PermissionUpdate};

/// The suggestions Ersa can offer the user, in the CLI's order, each with its
/// index in `updates` (the request's `permission_suggestions`).
///
/// The event's suggestion kinds are "allow a rule", "allow a directory", and
/// "set a mode". A CLI suggestion that is none of those (removing rules, say,
/// or adding a deny rule) has no place on the shelf and is left out, which is
/// why the event's indices and the CLI's can differ.
pub(crate) fn offered(updates: &[Value]) -> Vec<(usize, PermissionSuggestion)> {
    updates
        .iter()
        .enumerate()
        .filter_map(|(index, update)| Some((index, to_suggestion(update)?)))
        .collect()
}

fn to_suggestion(raw: &Value) -> Option<PermissionSuggestion> {
    let update: PermissionUpdate = serde_json::from_value(raw.clone()).ok()?;
    let (kind, label) = match &update {
        PermissionUpdate::AddRules {
            rules,
            behavior,
            destination,
        } if behavior == "allow" => (
            SuggestionKind::AllowRule,
            format!(
                "Always allow {} {}",
                rule_list(rules),
                where_to(destination)
            ),
        ),
        PermissionUpdate::AddDirectories {
            directories,
            destination,
        } => (
            SuggestionKind::AllowDirectory,
            format!(
                "Allow access to {} {}",
                directories.join(", "),
                where_to(destination)
            ),
        ),
        PermissionUpdate::SetMode { mode, destination } => (
            SuggestionKind::SetMode,
            format!("Switch to {mode} mode {}", where_to(destination)),
        ),
        _ => return None,
    };
    Some(PermissionSuggestion {
        kind,
        label,
        // The CLI's own object, untouched, so the driver can hand it back.
        payload: raw.clone(),
    })
}

/// `Bash(echo probe *)`, or just `Bash` for a rule covering the whole tool:
/// the notation Claude Code's own settings use.
fn rule_list(rules: &[PermissionRule]) -> String {
    let names: Vec<String> = rules
        .iter()
        .map(|rule| match &rule.rule_content {
            Some(content) => format!("{}({content})", rule.tool_name),
            None => rule.tool_name.clone(),
        })
        .collect();
    names.join(", ")
}

/// Where a suggestion would be saved, in words. The destinations are the
/// CLI's settings layers.
fn where_to(destination: &str) -> String {
    match destination {
        "session" | "cliArg" => "for this session".into(),
        "localSettings" => "in this project's local settings".into(),
        "projectSettings" => "in this project's settings".into(),
        "userSettings" => "in your user settings".into(),
        other => format!("({other})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn updates(value: Value) -> Vec<Value> {
        value.as_array().unwrap().clone()
    }

    #[test]
    fn offers_allow_rules_directories_and_modes_and_keeps_cli_indices() {
        let updates = updates(json!([
            {"type":"removeRules","rules":[{"toolName":"Bash"}],"behavior":"allow","destination":"session"},
            {"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"echo probe *"}],"behavior":"allow","destination":"localSettings","addedLater":true},
            {"type":"addDirectories","directories":["C:\\work"],"destination":"session"},
            {"type":"setMode","mode":"acceptEdits","destination":"session"}
        ]));
        let offered = offered(&updates);

        let summary: Vec<_> = offered
            .iter()
            .map(|(i, s)| (*i, s.kind, s.label.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    1,
                    SuggestionKind::AllowRule,
                    "Always allow Bash(echo probe *) in this project's local settings"
                ),
                (
                    2,
                    SuggestionKind::AllowDirectory,
                    "Allow access to C:\\work for this session"
                ),
                (
                    3,
                    SuggestionKind::SetMode,
                    "Switch to acceptEdits mode for this session"
                ),
            ]
        );
        // The payload is the CLI's object exactly as it arrived, including a
        // field the typed model does not know.
        assert_eq!(offered[0].1.payload, updates[1]);
        assert_eq!(offered[0].1.payload["addedLater"], true);
    }
}
