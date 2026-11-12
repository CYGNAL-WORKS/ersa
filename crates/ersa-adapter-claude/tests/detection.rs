//! What the Claude Code adapter tells Core about its backend: how it is
//! detected, the titles it takes, and which recordings are its own.
//!
//! The version command reads `ERSA_CLAUDE`. A test cannot safely set
//! variables in its own process (other tests run on other threads), so the
//! outer test runs this test binary again with the variable set, and the
//! inner test, ignored otherwise, does the check there.

mod common;

use ersa_adapter::recording::Recording;
use ersa_adapter::{Adapter, TitleRules};
use ersa_adapter_claude::ClaudeAdapter;
use serde_json::json;

const INNER: &str = "inner_the_version_command_runs_the_program_sessions_run";

/// The program the inner test expects, passed down beside `ERSA_CLAUDE`.
const EXPECTED: &str = "ERSA_TEST_EXPECTED_PROGRAM";

#[test]
fn the_version_command_finds_the_cli_as_sessions_do() {
    // An empty `ERSA_CLAUDE` counts as unset, as it does for a session.
    for (value, expected) in [
        ("", "claude"),
        ("C:/elsewhere/claude.exe", "C:/elsewhere/claude.exe"),
    ] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                INNER,
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("ERSA_CLAUDE", value)
            .env(EXPECTED, expected)
            .output()
            .expect("the test binary runs");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{value:?}: {stdout}");
        // The inner test ran, rather than being filtered away.
        assert!(
            stdout.contains(&format!("checked {expected}")),
            "{value:?}: {stdout}"
        );
    }
}

#[test]
#[ignore = "run by the_version_command_finds_the_cli_as_sessions_do"]
fn inner_the_version_command_runs_the_program_sessions_run() {
    // Run on its own (by `--ignored`, say), there is nothing to check.
    let Ok(expected) = std::env::var(EXPECTED) else {
        return;
    };
    let spec = ClaudeAdapter::new().version_command();
    assert_eq!(spec.program, expected);
    assert_eq!(spec.args, ["--version"]);
    println!("checked {expected}");
}

#[test]
fn the_version_is_the_first_word_the_cli_prints() {
    let adapter = ClaudeAdapter::new();
    let read = |line: &str| adapter.read_version(&[line.to_string()]);
    assert_eq!(read("2.1.316 (Claude Code)").as_deref(), Some("2.1.316"));
    assert_eq!(read("Claude Code 2.1"), None);
    assert_eq!(adapter.read_version(&[]), None);
}

// verifies: PROTO-CORE-API-R70
#[test]
fn a_title_is_one_line_of_at_most_200_characters() {
    assert_eq!(
        ClaudeAdapter::new().title_rules(),
        TitleRules {
            max_chars: 200,
            one_line: true
        }
    );
}

// verifies: PROTO-CORE-API-R69
#[test]
fn a_recording_is_its_own_when_its_notes_name_the_cli_version() {
    let adapter = ClaudeAdapter::new();
    let fixture = common::fixtures_root().join("2.1.316/00-session-tools.jsonl");
    let recording = Recording::load(&fixture).expect("the fixture loads");
    assert_eq!(
        adapter.recording_version(&recording).as_deref(),
        Ok("2.1.316")
    );
    // Notes that name no version, or a blank one, are not its own.
    for meta in [
        json!({}),
        json!({ "claude_code_version": "  " }),
        json!({ "claude_code_version": 2 }),
        json!({ "copilot_version": "1.0.0" }),
    ] {
        let recording = Recording::parse("", meta.clone()).unwrap();
        let reason = adapter.recording_version(&recording).unwrap_err();
        assert!(reason.contains("claude_code_version"), "{meta}: {reason}");
    }
}
