//! What a Claude Code tool call acts on and what it changes, read from the
//! call's `name` and `input` (PROTO-CLAUDE-CODE-STREAM, Tool targets and
//! edits), so `tool.requested` carries a `target` and `edits` and the
//! Interface never reads Claude Code's tool input to draw a card.

use ersa_protocol::events::FileEdit;
use serde_json::{Map, Value};

/// The `target` and `edits` a `tool.requested` carries for a call to `name`
/// with `input` (PROTO-CLAUDE-CODE-STREAM-R48). A tool the table does not name,
/// an MCP server's included, gets neither. A field missing from the input, or
/// not a string, leaves the target or that one edit out; a call left with no
/// edit carries none rather than an empty list.
pub(crate) fn target_and_edits(
    name: &str,
    input: &Map<String, Value>,
) -> (Option<String>, Option<Vec<FileEdit>>) {
    // The input field that holds the call's target, by tool. Matched exactly:
    // Claude Code's tool names are case-sensitive identifiers.
    let target_field = match name {
        "Read" | "Write" | "Edit" | "MultiEdit" => "file_path",
        "NotebookEdit" => "notebook_path",
        "Bash" => "command",
        "Grep" | "Glob" => "pattern",
        "WebFetch" => "url",
        "WebSearch" => "query",
        "Task" | "Agent" => "description",
        _ => return (None, None),
    };
    let target = string(input, target_field);

    let edits: Vec<FileEdit> = match name {
        // A whole file written: the new text is the file's content, and
        // nothing was replaced.
        "Write" => edit(&target, None, string(input, "content"))
            .into_iter()
            .collect(),
        "Edit" => edit(
            &target,
            Some(string(input, "old_string")),
            string(input, "new_string"),
        )
        .into_iter()
        .collect(),
        // One edit per entry, in the order the call makes them; an entry
        // missing a field is left out on its own.
        "MultiEdit" => input
            .get("edits")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .filter_map(|entry| {
                edit(
                    &target,
                    Some(string(entry, "old_string")),
                    string(entry, "new_string"),
                )
            })
            .collect(),
        // NotebookEdit changes cells, not text, so it carries no edits; every
        // other tool in the table changes no file.
        _ => Vec::new(),
    };
    (target, (!edits.is_empty()).then_some(edits))
}

/// The input's `field`, when it is a string.
fn string(input: &Map<String, Value>, field: &str) -> Option<String> {
    input.get(field).and_then(Value::as_str).map(str::to_owned)
}

/// One edit to `path`, or none when a field it needs is missing. `old_text`
/// is `None` for a whole file written and `Some(None)` for a replacement whose
/// old text is missing, which leaves the edit out.
fn edit(
    path: &Option<String>,
    old_text: Option<Option<String>>,
    new_text: Option<String>,
) -> Option<FileEdit> {
    let old_text = match old_text {
        None => None,
        Some(old) => Some(old?),
    };
    Some(FileEdit {
        path: path.clone()?,
        old_text,
        new_text: new_text?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn project(name: &str, input: Value) -> (Option<String>, Option<Vec<FileEdit>>) {
        match input {
            Value::Object(input) => target_and_edits(name, &input),
            other => panic!("not an object: {other}"),
        }
    }

    fn file_edit(path: &str, old: Option<&str>, new: &str) -> FileEdit {
        FileEdit {
            path: path.into(),
            old_text: old.map(Into::into),
            new_text: new.into(),
        }
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R48
    // verifies: PROTO-AGENT-EVENTS-R21
    #[test]
    fn each_tool_in_the_table_targets_its_own_field() {
        let cases = [
            (
                "Read",
                json!({ "file_path": "/w/a.rs", "limit": 10 }),
                "/w/a.rs",
            ),
            (
                "NotebookEdit",
                json!({ "notebook_path": "/w/n.ipynb", "new_source": "x" }),
                "/w/n.ipynb",
            ),
            (
                "Bash",
                json!({ "command": "cargo test\necho done", "description": "Run tests" }),
                "cargo test\necho done",
            ),
            (
                "Grep",
                json!({ "pattern": "fn main", "path": "/w" }),
                "fn main",
            ),
            (
                "Glob",
                json!({ "pattern": "**/*.rs", "path": "/w" }),
                "**/*.rs",
            ),
            (
                "WebFetch",
                json!({ "url": "https://example.com", "prompt": "Summarize" }),
                "https://example.com",
            ),
            ("WebSearch", json!({ "query": "tauri 2" }), "tauri 2"),
            (
                "Task",
                json!({ "description": "Count files", "prompt": "Count them" }),
                "Count files",
            ),
            (
                "Agent",
                json!({ "description": "Count files", "prompt": "Count them" }),
                "Count files",
            ),
        ];
        for (name, input, target) in cases {
            assert_eq!(
                project(name, input),
                (Some(target.to_owned()), None),
                "{name}"
            );
        }
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R48
    // verifies: PROTO-AGENT-EVENTS-R20
    #[test]
    fn a_call_that_changes_files_carries_one_edit_per_change() {
        assert_eq!(
            project(
                "Edit",
                json!({ "file_path": "/w/a.md", "old_string": "old", "new_string": "new", "replace_all": true })
            ),
            (
                Some("/w/a.md".into()),
                Some(vec![file_edit("/w/a.md", Some("old"), "new")])
            )
        );
        // A whole file written keeps no replaced text.
        assert_eq!(
            project(
                "Write",
                json!({ "file_path": "/w/b.txt", "content": "Hello\n" })
            ),
            (
                Some("/w/b.txt".into()),
                Some(vec![file_edit("/w/b.txt", None, "Hello\n")])
            )
        );
        assert_eq!(
            project(
                "MultiEdit",
                json!({ "file_path": "/w/c.rs", "edits": [
                    { "old_string": "a", "new_string": "b" },
                    { "old_string": "c", "new_string": "d", "replace_all": true },
                ]})
            ),
            (
                Some("/w/c.rs".into()),
                Some(vec![
                    file_edit("/w/c.rs", Some("a"), "b"),
                    file_edit("/w/c.rs", Some("c"), "d"),
                ])
            )
        );
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R48
    // verifies: PROTO-AGENT-EVENTS-R20
    // verifies: PROTO-AGENT-EVENTS-R21
    #[test]
    fn a_tool_the_table_does_not_name_carries_neither() {
        for name in [
            "PowerShell",
            "ToolSearch",
            "mcp__docs__search",
            "edit",
            "TodoWrite",
            "AskUserQuestion",
        ] {
            assert_eq!(
                project(
                    name,
                    json!({ "file_path": "/w/a", "command": "ls", "content": "x" })
                ),
                (None, None),
                "{name}"
            );
        }
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R48
    #[test]
    fn a_missing_or_non_string_field_leaves_that_part_out() {
        assert_eq!(project("Read", json!({})), (None, None));
        assert_eq!(project("Bash", json!({ "command": ["ls"] })), (None, None));
        // No path: the target and the edit both go.
        assert_eq!(project("Write", json!({ "content": "x" })), (None, None));
        // A path but no new text: the target stays, the edit goes.
        assert_eq!(
            project("Edit", json!({ "file_path": "/w/a", "old_string": "x" })),
            (Some("/w/a".into()), None)
        );
        assert_eq!(
            project("Edit", json!({ "file_path": "/w/a", "new_string": "x" })),
            (Some("/w/a".into()), None)
        );
        // Only the broken entry of a MultiEdit is left out.
        assert_eq!(
            project(
                "MultiEdit",
                json!({ "file_path": "/w/c", "edits": [
                    { "old_string": "a" },
                    "not an entry",
                    { "old_string": "c", "new_string": 4 },
                    { "old_string": "e", "new_string": "f" },
                ]})
            ),
            (
                Some("/w/c".into()),
                Some(vec![file_edit("/w/c", Some("e"), "f")])
            )
        );
    }
}
