//! Tests against the real, installed Claude Code CLI: one tiny prompt with a
//! permission round trip, using the machine's own Claude login; and which of a
//! workspace's configuration a session loads, trusted or not, under a scratch
//! configuration folder that is logged in nowhere (so it costs no model call).
//!
//! Ignored by default because they need the CLI, and the first costs a model
//! call and needs a logged-in CLI. Run them by hand:
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
use ersa_adapter::{
    Adapter, LocalTransport, PermissionDecision, Process, ProcessSpec, SessionConfig, Transport,
    async_trait,
};
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
        effort: None,
        project_trusted: false,
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

/// The local transport, with Claude Code pointed at a configuration folder of
/// the test's own: its user settings and MCP servers, and no login.
struct ScratchConfig(PathBuf);

#[async_trait]
impl Transport for ScratchConfig {
    async fn spawn(&self, mut spec: ProcessSpec) -> std::io::Result<Box<dyn Process>> {
        spec.env.insert(
            "CLAUDE_CONFIG_DIR".into(),
            self.0.to_string_lossy().into_owned(),
        );
        // Whatever credential or provider the machine has would make the
        // turn a model call.
        spec.env_remove.extend(
            [
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "ANTHROPIC_BASE_URL",
                "CLAUDE_CODE_OAUTH_TOKEN",
                "CLAUDE_CODE_USE_BEDROCK",
                "CLAUDE_CODE_USE_VERTEX",
                "CLAUDE_CODE_USE_FOUNDRY",
            ]
            .map(String::from),
        );
        LocalTransport.spawn(spec).await
    }
}

/// A `SessionStart` hook that leaves `mark` in the session's folder.
fn marking_hook(mark: &str) -> Value {
    json!({ "hooks": { "SessionStart": [{ "hooks": [
        { "type": "command", "command": format!("echo x > {mark}") }
    ]}]}})
}

/// What a session loaded: the marks its hooks left, the MCP servers
/// `system/init` named, and which of the workspace's own skill, agent and
/// command it offered.
#[derive(Debug)]
struct Loaded {
    marks: Vec<String>,
    servers: Vec<String>,
    workspace_extras: Vec<String>,
}

/// The workspace's skill, agent and command, by the names `system/init`
/// would list them under.
const WORKSPACE_EXTRAS: [&str; 3] = ["ws-skill", "ws-agent", "ws-command"];

/// Starts a session in `repo`, sends one message (the turn fails at once,
/// logged in nowhere), and returns what it loaded.
async fn workspace_configuration_loaded(
    config_dir: &std::path::Path,
    repo: &std::path::Path,
    trusted: bool,
) -> Loaded {
    for mark in ["mark-user.txt", "mark-project.txt", "mark-local.txt"] {
        let _ = fs::remove_file(repo.join(mark));
    }
    let config = SessionConfig {
        cwd: repo.to_string_lossy().into_owned(),
        permission_mode: "default".into(),
        model: None,
        effort: None,
        project_trusted: trusted,
        extra: json!({ "claude_path": claude_path() }),
    };
    let sink = Arc::new(RecordingSink::new());
    let session = ClaudeAdapter::new()
        .start(
            Arc::new(ScratchConfig(config_dir.to_path_buf())),
            config,
            sink.clone(),
        )
        .await
        .expect("start: is the CLI installed?");
    session.send_user("hello").await.expect("send");
    let init = sink
        .wait_for_line(TURN, |l| is_kind(l, "system/init"))
        .await;
    sink.wait_for_line(TURN, |l| is_kind(l, "result")).await;
    session.stop().await.expect("stop");
    sink.wait_for_exit(Duration::from_secs(10)).await;

    let init: Value = serde_json::from_str(&init.line).unwrap();
    let servers = init["mcp_servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap().to_string())
        .collect();
    let marks = ["mark-user.txt", "mark-project.txt", "mark-local.txt"]
        .into_iter()
        .filter(|mark| repo.join(mark).is_file())
        .map(String::from)
        .collect();
    // Skills and commands are offered as slash commands, agents by name.
    let offered: Vec<&str> = ["skills", "agents", "slash_commands"]
        .into_iter()
        .flat_map(|key| init[key].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .collect();
    let workspace_extras = WORKSPACE_EXTRAS
        .into_iter()
        .filter(|name| offered.contains(name))
        .map(String::from)
        .collect();
    Loaded {
        marks,
        servers,
        workspace_extras,
    }
}

/// Against a scratch repository whose settings run hooks, whose `.mcp.json`
/// names a server and which has a skill, an agent and a command of its own,
/// with user settings that enable every project server: an untrusted
/// project's session loads only the user's own hook and server and none of
/// the workspace's, a trusted one's loads the workspace's too.
// verifies: PROTO-CLAUDE-CODE-STREAM-R12
// verifies: PROTO-CLAUDE-CODE-STREAM-R18
#[tokio::test]
#[ignore = "drives the installed Claude Code CLI; run by hand"]
async fn only_a_trusted_projects_workspace_configuration_loads_in_the_real_cli() {
    let dir = std::env::temp_dir().join(format!("ersa-trust-{}", std::process::id()));
    let config_dir = dir.join("config");
    let repo = dir.join("repo");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(repo.join(".claude")).unwrap();
    let mut user = marking_hook("mark-user.txt");
    user["enableAllProjectMcpServers"] = json!(true);
    fs::write(config_dir.join("settings.json"), user.to_string()).unwrap();
    let servers =
        |name: &str| json!({ "mcpServers": { name: { "command": "ersa-no-such-mcp-server" } } });
    fs::write(
        config_dir.join(".claude.json"),
        servers("user-server").to_string(),
    )
    .unwrap();
    fs::write(
        repo.join(".claude/settings.json"),
        marking_hook("mark-project.txt").to_string(),
    )
    .unwrap();
    fs::write(
        repo.join(".claude/settings.local.json"),
        marking_hook("mark-local.txt").to_string(),
    )
    .unwrap();
    fs::write(
        repo.join(".mcp.json"),
        servers("workspace-server").to_string(),
    )
    .unwrap();

    let described =
        |name: &str| format!("---\nname: {name}\ndescription: the workspace's own\n---\nhello\n");
    fs::create_dir_all(repo.join(".claude/skills/ws-skill")).unwrap();
    fs::write(
        repo.join(".claude/skills/ws-skill/SKILL.md"),
        described("ws-skill"),
    )
    .unwrap();
    fs::create_dir_all(repo.join(".claude/agents")).unwrap();
    fs::write(
        repo.join(".claude/agents/ws-agent.md"),
        described("ws-agent"),
    )
    .unwrap();
    fs::create_dir_all(repo.join(".claude/commands")).unwrap();
    fs::write(repo.join(".claude/commands/ws-command.md"), "hello\n").unwrap();

    let untrusted = workspace_configuration_loaded(&config_dir, &repo, false).await;
    assert_eq!(
        untrusted.marks,
        ["mark-user.txt"],
        "untrusted: only the user's hook"
    );
    assert_eq!(
        untrusted.servers,
        ["user-server"],
        "untrusted: only the user's server"
    );
    assert!(
        untrusted.workspace_extras.is_empty(),
        "untrusted: no workspace skill, agent or command: {:?}",
        untrusted.workspace_extras
    );

    let mut trusted = workspace_configuration_loaded(&config_dir, &repo, true).await;
    assert_eq!(
        trusted.marks,
        ["mark-user.txt", "mark-project.txt", "mark-local.txt"],
        "trusted: every hook"
    );
    trusted.servers.sort();
    assert_eq!(trusted.servers, ["user-server", "workspace-server"]);
    // The check above means something only if the CLI does offer them.
    assert_eq!(
        trusted.workspace_extras, WORKSPACE_EXTRAS,
        "trusted: {trusted:?}"
    );

    let _ = fs::remove_dir_all(&dir);
}
