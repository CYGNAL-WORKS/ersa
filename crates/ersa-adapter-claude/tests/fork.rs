//! Forking a session, from fixtures 2.1.316/13 to 19: what the CLI does with
//! `--resume <id> --fork-session`, the invocation the adapter makes of it,
//! and how a record says a conversation ends (PROTO-CLAUDE-CODE-STREAM,
//! Invocation; FLOW-FORK-SESSION).

mod common;

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use common::{RecordingSink, fixture, fixtures_root};
use ersa_adapter::ForkOf;
use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{Adapter, AdapterError, ConversationEnd, SessionConfig};
use ersa_adapter_claude::record::{
    LiveCheck, conversation_end, conversation_end_from, encoded_cwd, live_session,
};
use ersa_adapter_claude::{ClaudeAdapter, fork_args};
use serde_json::{Value, json};

const ORIGINAL: &str = "67e5f4e2-f63e-48f6-a938-1c8f53780b27";
const FORK: &str = "7830632d-af9a-4a3a-a63f-b70ee3de8f31";

fn parse(line: &str) -> Value {
    serde_json::from_str(line).unwrap()
}

fn meta(name: &str) -> Value {
    let path = fixtures_root().join(format!("2.1.316/{name}.meta.json"));
    parse(&fs::read_to_string(path).unwrap())
}

fn record(name: &str) -> String {
    fs::read_to_string(fixtures_root().join(format!("2.1.316/record/{name}.jsonl"))).unwrap()
}

/// The `uuid`s of a record's user and assistant entries, in order.
fn messages(text: &str) -> Vec<String> {
    text.lines()
        .map(parse)
        .filter(|e| matches!(e["type"].as_str(), Some("user" | "assistant")))
        .map(|e| e["uuid"].as_str().unwrap().to_string())
        .collect()
}

fn end_of(text: &str) -> ConversationEnd {
    let mut reader = std::io::Cursor::new(text.as_bytes());
    conversation_end_from(&mut reader, text.len() as u64).unwrap()
}

/// The first `lines` lines of a record.
fn first_lines(text: &str, lines: usize) -> String {
    let kept: Vec<&str> = text.lines().take(lines).collect();
    format!("{}\n", kept.join("\n"))
}

/// A record entry that follows `parent`.
fn entry(uuid: &str, parent: &str, fields: Value) -> String {
    let mut line = fields;
    line["uuid"] = json!(uuid);
    line["parentUuid"] = json!(parent);
    format!("{line}\n")
}

fn prompt(text: &str) -> Value {
    json!({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": text}]}})
}

/// An assistant entry written by `model`, ended by `stop_reason`.
fn reply(model: &str, stop_reason: Value) -> Value {
    json!({"type": "assistant", "message": {
        "id": "msg_1", "type": "message", "role": "assistant", "model": model,
        "stop_reason": stop_reason, "content": [{"type": "text", "text": "done"}]
    }})
}

/// A conversation of one finished reply, and its last entry's id.
fn finished() -> (String, &'static str) {
    let first =
        entry("a", "", prompt("hello")).replace(r#""parentUuid":"""#, r#""parentUuid":null"#);
    (
        first + &entry("b", "a", reply("claude-opus-4-8", json!("end_turn"))),
        "b",
    )
}

#[test]
fn the_recorded_fork_has_a_new_id_and_leaves_the_original_record_as_it_was() {
    for name in [
        "14-fork-session",
        "16-fork-named-before-first-message",
        "17-fork-record-ends-at-tool-use",
        "18-fork-record-ends-at-prompt",
    ] {
        let meta = meta(name);
        let original = &meta["fork_original"];
        assert_eq!(original["session_id"], ORIGINAL, "{name}");
        assert_ne!(meta["session_id"], ORIGINAL, "{name}");
        assert_eq!(original["before"], original["after"], "{name}");
    }
    // The copy is the conversation under the new id, then the new turn.
    let copy = record("14-fork-session");
    let fork_id = meta("14-fork-session")["session_id"].clone();
    for entry in copy.lines().map(parse).filter(|e| e["type"] == "user") {
        assert_eq!(entry["sessionId"], fork_id);
    }
    let original = messages(&record("13-fork-original"));
    assert_eq!(messages(&copy)[..original.len()], original[..]);
}

#[test]
fn a_chosen_id_names_the_fork_and_the_cli_writes_nothing_before_its_first_message() {
    let named = meta("16-fork-named-before-first-message");
    let args: Vec<&str> = named["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    let chosen = args[args.iter().position(|a| *a == "--session-id").unwrap() + 1];
    assert_eq!(chosen, FORK);
    assert_eq!(named["session_id"], FORK);
    assert!(args.contains(&"--fork-session"));
    // Renamed and closed without a message: the CLI took the rename and
    // kept no record.
    let unsent = meta("15-fork-named-no-message");
    assert_eq!(unsent["session_record"], Value::Null);
    assert_eq!(unsent["turns"][0]["subtype"], "success");
    assert_eq!(
        unsent["fork_original"]["before"],
        unsent["fork_original"]["after"]
    );
}

#[test]
fn a_part_way_record_gets_a_made_up_end_in_the_copy() {
    // Why Core refuses it: a record cut off at a tool call or a prompt
    // becomes, in the copy, a reply nobody wrote.
    for name in [
        "17-fork-record-ends-at-tool-use",
        "18-fork-record-ends-at-prompt",
    ] {
        let copy = record(name);
        let made_up = copy
            .lines()
            .map(parse)
            .find(|e| e["message"]["model"] == "<synthetic>")
            .unwrap_or_else(|| panic!("{name} has no synthetic reply"));
        assert_eq!(
            made_up["message"]["content"][0]["text"],
            "No response requested."
        );
    }
    assert!(record("17-fork-record-ends-at-tool-use").contains("Tool call interrupted"));
}

#[test]
fn the_cli_copies_the_originals_title_as_it_stands() {
    let title = record("19-fork-of-titled")
        .lines()
        .map(parse)
        .find(|e| e["type"] == "custom-title")
        .expect("a title");
    assert_eq!(title["customTitle"], "Titled original");
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R38
#[test]
fn fork_args_name_the_original_and_ask_for_a_fork() {
    let config = fork_of(ORIGINAL);
    assert_eq!(
        fork_args(&config).unwrap(),
        ["--resume", ORIGINAL, "--fork-session"]
    );
    assert!(fork_args(&SessionConfig::default()).unwrap().is_empty());
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R45
#[test]
fn a_titled_sessions_fork_is_named_at_launch() {
    let config = SessionConfig {
        fork_of: Some(ForkOf {
            original: ORIGINAL.into(),
            name: Some("Titled (fork)".into()),
        }),
        ..SessionConfig::default()
    };
    assert_eq!(
        fork_args(&config).unwrap(),
        [
            "--resume",
            ORIGINAL,
            "--fork-session",
            "--name",
            "Titled (fork)"
        ]
    );
}

/// A config that forks `original`, with no name.
fn fork_of(original: &str) -> SessionConfig {
    SessionConfig {
        fork_of: Some(ForkOf {
            original: original.into(),
            name: None,
        }),
        ..SessionConfig::default()
    }
}

#[test]
fn a_fork_of_something_that_is_no_session_id_never_reaches_the_command_line() {
    let config = fork_of("--dangerously");
    assert!(matches!(fork_args(&config), Err(AdapterError::Protocol(_))));
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R38
#[tokio::test(flavor = "multi_thread")]
async fn the_adapter_forks_in_the_stored_path_under_the_id_core_chose() {
    let stdout = fixture("2.1.316/16-fork-named-before-first-message").stdout;
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(|_| Ok(()))
        .then_stdout([&stdout[0]])
        .hold_open();
    let adapter = ClaudeAdapter::new()
        .with_handshake_timeout(Duration::from_secs(5))
        .with_stop_grace(Duration::from_millis(200));
    let config = SessionConfig {
        cwd: "C:/work".into(),
        permission_mode: "default".into(),
        backend_session_id: Some(FORK.into()),
        fork_of: fork_of(ORIGINAL).fork_of,
        ..SessionConfig::default()
    };
    let sink = Arc::new(RecordingSink::new());
    let session = adapter
        .start(Arc::new(transport.clone()), config, sink)
        .await
        .unwrap();
    let spawned = &transport.spawned()[0];
    assert_eq!(spawned.cwd.as_deref(), Some("C:/work"));
    let tail: Vec<&str> = spawned
        .args
        .iter()
        .rev()
        .take(5)
        .rev()
        .map(String::as_str)
        .collect();
    assert_eq!(
        tail,
        ["--resume", ORIGINAL, "--fork-session", "--session-id", FORK]
    );
    session.stop().await.unwrap();
}

// verifies: FLOW-FORK-SESSION-R03
#[test]
fn a_finished_reply_ends_the_conversation_whatever_bookkeeping_follows() {
    // Fixture 13 ends with `last-prompt` and `cost-state` entries.
    assert_eq!(
        end_of(&record("13-fork-original")),
        ConversationEnd::Finished
    );
    assert_eq!(
        end_of(&record("14-fork-session")),
        ConversationEnd::Finished
    );
}

// verifies: FLOW-FORK-SESSION-R03
#[test]
fn a_record_cut_part_way_through_a_turn_does_not_end_with_a_finished_reply() {
    let original = record("13-fork-original");
    // Lines 26 and 18 are where fixtures 17 and 18 cut it: the first turn,
    // at its tool call and at its prompt, so no reply has finished.
    assert_eq!(
        end_of(&first_lines(&original, 18)),
        ConversationEnd::NoReply
    );
    assert_eq!(
        end_of(&first_lines(&original, 26)),
        ConversationEnd::NoReply
    );
    // The second turn's prompt, after a finished reply.
    let copy = record("14-fork-session");
    let prompt_at = copy
        .lines()
        .position(|l| l.contains("Reply with the single word: forked"))
        .unwrap();
    assert_eq!(
        end_of(&first_lines(&copy, prompt_at + 1)),
        ConversationEnd::Unfinished
    );
}

// verifies: FLOW-FORK-SESSION-R03
#[test]
fn a_finished_reply_followed_by_a_tool_call_is_unfinished() {
    let (done, last) = finished();
    let text = done + &entry("c", last, reply("claude-opus-4-8", json!("tool_use")));
    assert_eq!(end_of(&text), ConversationEnd::Unfinished);
}

// verifies: FLOW-FORK-SESSION-R03
#[test]
fn an_interrupted_turn_is_interrupted() {
    let (done, last) = finished();
    let marker = done.clone() + &entry("c", last, prompt("[Request interrupted by user]"));
    assert_eq!(end_of(&marker), ConversationEnd::Interrupted);
    let tool = done.clone()
        + &entry(
            "c",
            last,
            prompt("[Request interrupted by user for tool use]"),
        );
    assert_eq!(end_of(&tool), ConversationEnd::Interrupted);
    // The CLI's stand-in for a reply is not one.
    let synthetic = done + &entry("c", last, reply("<synthetic>", json!("stop_sequence")));
    assert_eq!(end_of(&synthetic), ConversationEnd::Interrupted);
}

// verifies: FLOW-FORK-SESSION-R03
#[test]
fn only_a_reply_that_ended_its_turn_is_finished() {
    let (done, last) = finished();
    for (stop_reason, end) in [
        (json!("end_turn"), ConversationEnd::Finished),
        (json!("max_tokens"), ConversationEnd::Finished),
        (json!("pause_turn"), ConversationEnd::Unfinished),
        (json!(null), ConversationEnd::Unfinished),
    ] {
        let text = done.clone() + &entry("c", last, reply("claude-opus-4-8", stop_reason));
        assert_eq!(end_of(&text), end);
    }
}

// verifies: FLOW-FORK-SESSION-R03
#[test]
fn what_the_cli_writes_beside_the_conversation_is_passed_over() {
    let (done, last) = finished();
    let mut command = prompt("<command-name>/rename</command-name>");
    command["isMeta"] = json!(false);
    let local = json!({"type": "system", "subtype": "local_command",
        "content": "<local-command-stdout>Renamed</local-command-stdout>"});
    let text = done.clone() + &entry("c", last, command) + &entry("d", "c", local);
    assert_eq!(end_of(&text), ConversationEnd::Finished);

    let mut summary = prompt("This session is being continued");
    summary["isCompactSummary"] = json!(true);
    let compacted = done.clone() + &entry("c", last, summary);
    assert_eq!(end_of(&compacted), ConversationEnd::Finished);

    // A background task's report starts a turn: its reply is still to come.
    let report = done
        + &entry(
            "c",
            last,
            prompt("<task-notification>done</task-notification>"),
        );
    assert_eq!(end_of(&report), ConversationEnd::Unfinished);
}

// verifies: FLOW-FORK-SESSION-R03
#[test]
fn a_half_written_last_line_is_a_turn_being_written() {
    let (done, _) = finished();
    let torn = done + r#"{"type":"user","uuid":"c","par"#;
    assert_eq!(end_of(&torn), ConversationEnd::Unfinished);
}

// verifies: FLOW-FORK-SESSION-R03
#[test]
fn an_abandoned_branch_does_not_count() {
    // A rewind in the terminal: the last line sits on the live chain, so
    // the unanswered prompt on the old branch is not the conversation's end.
    let (done, _) = finished();
    let old = entry("c", "b", prompt("abandoned"));
    let live = entry("d", "a", reply("claude-opus-4-8", json!("end_turn")));
    assert_eq!(end_of(&(done + &old + &live)), ConversationEnd::Finished);
}

// verifies: FLOW-FORK-SESSION-R03
#[test]
fn no_record_is_no_reply_yet() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(
        conversation_end(home.path(), "C:/work", ORIGINAL).unwrap(),
        ConversationEnd::NoReply
    );
    let dir = home.path().join("projects").join(encoded_cwd("C:/work"));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(format!("{ORIGINAL}.jsonl")),
        record("13-fork-original"),
    )
    .unwrap();
    assert_eq!(
        conversation_end(home.path(), "C:/work", ORIGINAL).unwrap(),
        ConversationEnd::Finished
    );
}

#[test]
fn the_registry_names_every_process_that_has_the_session_open() {
    let home = tempfile::tempdir().unwrap();
    let sessions = home.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    // Two files naming this test's own process, which is known to run.
    let pid = std::process::id();
    for name in [1u32, 2] {
        fs::write(
            sessions.join(format!("{name}.json")),
            json!({ "pid": pid, "sessionId": ORIGINAL }).to_string(),
        )
        .unwrap();
    }
    let LiveCheck::Open(open) = live_session(home.path(), ORIGINAL) else {
        panic!("not reported");
    };
    assert_eq!(open.iter().map(|l| l.pid).collect::<Vec<_>>(), [pid, pid]);
}

// verifies: FLOW-FORK-SESSION-R03
#[test]
fn a_command_with_no_output_after_it_is_a_turn_still_to_be_answered() {
    let (done, last) = finished();
    // A skill: the echo, then its expansion, and the reply never came.
    let mut expansion = prompt("Do the thing the skill says");
    expansion["isMeta"] = json!(true);
    let text = done
        + &entry("c", last, prompt("<command-name>/skill</command-name>"))
        + &entry("d", "c", expansion);
    assert_eq!(end_of(&text), ConversationEnd::Unfinished);
}

// verifies: FLOW-FORK-SESSION-R03
#[test]
fn a_subagents_entries_are_not_the_conversations_end() {
    let (done, _) = finished();
    let mut sidechain = prompt("a subagent's prompt");
    sidechain["isSidechain"] = json!(true);
    let text = done + &entry("c", "b", sidechain);
    assert_eq!(end_of(&text), ConversationEnd::Finished);
}

#[test]
fn a_fork_is_not_also_a_resume() {
    let config = SessionConfig {
        fork_of: fork_of(ORIGINAL).fork_of,
        resume: Some(ORIGINAL.into()),
        ..SessionConfig::default()
    };
    assert!(matches!(fork_args(&config), Err(AdapterError::Protocol(_))));
}

#[test]
fn a_fork_started_in_another_folder_finds_the_original_and_leaves_it_as_it_was() {
    let meta = meta("21-fork-into-worktree-renamed");
    let args: Vec<&str> = meta["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert!(args.contains(&"--fork-session"));
    let chosen = args[args.iter().position(|a| *a == "--session-id").unwrap() + 1];
    assert_eq!(meta["session_id"], chosen);
    assert_eq!(
        meta["fork_original"]["before"],
        meta["fork_original"]["after"]
    );
    // The conversation came with it, into the fork's own record.
    let copy = record("21-fork-into-worktree-renamed");
    let original = messages(&record("20-fork-of-worktree-original"));
    assert_eq!(messages(&copy)[..original.len()], original[..]);
}

#[test]
fn a_fork_is_renamed_over_the_title_it_copied() {
    let copy = record("21-fork-into-worktree-renamed");
    let titles: Vec<String> = copy
        .lines()
        .map(parse)
        .filter(|e| e["type"] == "custom-title")
        .map(|e| e["customTitle"].as_str().unwrap().to_string())
        .collect();
    assert!(!titles.is_empty());
    assert!(
        titles.iter().all(|t| t == "Titled original (fork)"),
        "{titles:?}"
    );
}
