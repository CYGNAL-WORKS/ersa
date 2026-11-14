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
        "Bash" | "PowerShell" => "command",
        "Grep" | "Glob" => "pattern",
        "WebFetch" => "url",
        "WebSearch" | "ToolSearch" => "query",
        "Task" | "Agent" => "description",
        _ => return (None, None),
    };
    let target = string(input, target_field);

    let edits: Vec<FileEdit> = match name {
        // A whole file written: the new text is the file's content, and
        // nothing was replaced.
        "Write" => whole_file(&target, input).into_iter().collect(),
        "Edit" => replacement(&target, input).into_iter().collect(),
        // One edit per entry, in the order the call makes them; an entry
        // missing a field is left out on its own.
        "MultiEdit" => input
            .get("edits")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .filter_map(|entry| replacement(&target, entry))
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

/// A `Write` call's one edit: the whole file, so no text replaced. None when
/// the path or the content is missing.
fn whole_file(path: &Option<String>, input: &Map<String, Value>) -> Option<FileEdit> {
    Some(FileEdit {
        path: path.clone()?,
        old_text: None,
        new_text: string(input, "content")?,
        every_occurrence: false,
    })
}

/// An `Edit` call's edit, or one entry of a `MultiEdit`'s: `old_string`
/// replaced by `new_string`, everywhere in the file when `replace_all` is true
/// (PROTO-AGENT-EVENTS-R23). None when the path or either string is missing.
fn replacement(path: &Option<String>, fields: &Map<String, Value>) -> Option<FileEdit> {
    Some(FileEdit {
        path: path.clone()?,
        old_text: Some(string(fields, "old_string")?),
        new_text: string(fields, "new_string")?,
        // Only a boolean true counts; anything else is the CLI's default,
        // which replaces one occurrence.
        every_occurrence: fields.get("replace_all") == Some(&Value::Bool(true)),
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
            every_occurrence: false,
        }
    }

    fn everywhere(edit: FileEdit) -> FileEdit {
        FileEdit {
            every_occurrence: true,
            ..edit
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
            (
                "PowerShell",
                json!({ "command": "Get-ChildItem -Name", "description": "List" }),
                "Get-ChildItem -Name",
            ),
            ("WebSearch", json!({ "query": "tauri 2" }), "tauri 2"),
            (
                "ToolSearch",
                json!({ "query": "select:WebSearch", "max_results": 1 }),
                "select:WebSearch",
            ),
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
                Some(vec![everywhere(file_edit("/w/a.md", Some("old"), "new"))])
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
                    everywhere(file_edit("/w/c.rs", Some("c"), "d")),
                ])
            )
        );
    }

    // verifies: PROTO-AGENT-EVENTS-R23
    #[test]
    fn only_a_change_made_everywhere_says_every_occurrence() {
        let edit = |input: Value| project("Edit", input).1.expect("an edit");
        let base = json!({ "file_path": "/w/a", "old_string": "x", "new_string": "y" });
        let with = |replace_all: Value| {
            let mut input = base.clone();
            input["replace_all"] = replace_all;
            input
        };
        assert!(edit(with(json!(true)))[0].every_occurrence);
        // Absent, false, or not a boolean: one occurrence, the CLI's default.
        for input in [base.clone(), with(json!(false)), with(json!("true"))] {
            assert!(!edit(input.clone())[0].every_occurrence, "{input}");
        }
        // A whole file written is not a replacement.
        let written = project(
            "Write",
            json!({ "file_path": "/w/a", "content": "x", "replace_all": true }),
        );
        assert!(!written.1.expect("an edit")[0].every_occurrence);
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R48
    // verifies: PROTO-AGENT-EVENTS-R20
    // verifies: PROTO-AGENT-EVENTS-R21
    #[test]
    fn a_tool_the_table_does_not_name_carries_neither() {
        for name in ["mcp__docs__search", "edit", "TodoWrite", "AskUserQuestion"] {
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
