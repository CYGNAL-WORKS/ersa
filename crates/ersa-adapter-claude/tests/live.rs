//! A smoke test against the real, installed Claude Code CLI: one tiny prompt
//! with a permission round trip, using the machine's own Claude login.
//!
//! Ignored by default because it costs a model call and needs a logged-in CLI.
//! Run it by hand:
//!
//! ```text
//! cargo test -p ersa-adapter-claude --test live -- --ignored --nocapture
//! ```
//!
//! It finds the CLI through `ERSA_CLAUDE`, else the winget install under
//! `%LOCALAPPDATA%\Microsoft\WinGet\Packages\Anthropic.ClaudeCode_*`, else
//! `claude` on `PATH`. `ERSA_SMOKE_MODEL` picks the model (default `haiku`, to
//! keep it cheap).

mod common;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use common::{RecordingSink, events, is_kind};
use ersa_adapter::{Adapter, LocalTransport, PermissionDecision, SessionConfig};
use ersa_adapter_claude::ClaudeAdapter;
use ersa_protocol::events::{Event, PermissionBehavior, TurnOutcome};
use ersa_protocol::schema::validate_event;
use serde_json::{Value, json};

const TURN: Duration = Duration::from_secs(180);

/// The installed CLI, as the module doc describes.
fn claude_path() -> String {
    if let Ok(path) = std::env::var("ERSA_CLAUDE") {
        return path;
    }
    let winget = std::env::var("LOCALAPPDATA")
        .map(|dir| PathBuf::from(dir).join("Microsoft/WinGet/Packages"))
        .ok()
        .and_then(|packages| fs::read_dir(packages).ok())
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("Anthropic.ClaudeCode_"))
        })
        .map(|package| package.join("claude.exe"))
        .filter(|exe| exe.is_file());
    match winget {
        Some(exe) => exe.to_string_lossy().into_owned(),
        None => "claude".into(),
    }
}

#[tokio::test]
#[ignore = "drives the installed Claude Code CLI with the user's login; run by hand"]
async fn a_tiny_prompt_with_a_permission_round_trip_against_the_real_cli() {
    // An empty working directory of its own, so the tool call touches nothing.
    let cwd = std::env::temp_dir().join(format!("ersa-smoke-{}", std::process::id()));
    fs::create_dir_all(&cwd).unwrap();
    let model = std::env::var("ERSA_SMOKE_MODEL").unwrap_or_else(|_| "haiku".into());
    let config = SessionConfig {
        cwd: cwd.to_string_lossy().into_owned(),
        permission_mode: "default".into(),
        model: Some(model),
        extra: json!({ "claude_path": claude_path() }),
    };

    let sink = Arc::new(RecordingSink::new());
    let session = ClaudeAdapter::new()
        .start(Arc::new(LocalTransport), config, sink.clone())
        .await
        .expect("start: is the CLI installed and logged in?");

    // Writing a file needs approval in the default mode, so this prompts.
    session
        .send_user(
            "Use the Write tool to create a file named smoke.txt containing the word ok. \
             Then reply with the single word: done",
        )
        .await
        .expect("send");
    let ask = sink
        .wait_for_line(TURN, |l| {
            is_kind(l, "control_request/can_use_tool") || is_kind(l, "result")
        })
        .await;
    assert!(
        is_kind(&ask, "control_request/can_use_tool"),
        "the turn ended without asking: {}",
        ask.line
    );
    let request_id = serde_json::from_str::<Value>(&ask.line).unwrap()["request_id"]
        .as_str()
        .unwrap()
        .to_string();
    session
        .decide_permission(
            &request_id,
            PermissionDecision {
                behavior: PermissionBehavior::Allow,
                message: None,
                updated_input: None,
                apply_suggestions: vec![],
            },
        )
        .await
        .expect("decide");
    sink.wait_for_line(TURN, |l| is_kind(l, "result")).await;
    session.stop().await.expect("stop");
    sink.wait_for_exit(Duration::from_secs(10)).await;

    let events = events(&sink.lines());
    for event in &events {
        println!("{}", serde_json::to_string(event).unwrap());
        validate_event(&serde_json::to_value(event).unwrap()).expect("schema");
    }
    let has = |wanted: &dyn Fn(&Event) -> bool| events.iter().any(wanted);
    assert!(has(&|e| matches!(e, Event::SessionStarted(_))));
    assert!(has(
        &|e| matches!(e, Event::PermissionRequested(r) if r.request_id == request_id)
    ));
    assert!(has(
        &|e| matches!(e, Event::PermissionDecided(d) if d.behavior == PermissionBehavior::Allow)
    ));
    assert!(has(&|e| matches!(e, Event::ToolResult(r) if !r.is_error)));
    assert!(has(
        &|e| matches!(e, Event::TurnCompleted(t) if t.outcome == TurnOutcome::Success)
    ));
    assert!(
        cwd.join("smoke.txt").is_file(),
        "the approved write happened"
    );
    assert_eq!(sink.exit_codes(), [Some(0)]);

    let _ = fs::remove_dir_all(&cwd);
}
