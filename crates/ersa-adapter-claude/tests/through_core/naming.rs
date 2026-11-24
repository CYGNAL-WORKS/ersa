//! A new Claude Code session's id and name, through Core's API with the real
//! Claude adapter and a stub CLI replaying fixture
//! 2.1.316/09-session-id-rename: Core chooses and stores the session's id
//! before the CLI starts (`--session-id`), and a rename while the session
//! starts waits for the CLI to answer `initialize`, then goes as
//! `rename_session` (PROTO-CLAUDE-CODE-STREAM, Invocation and Renaming).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::claude::{OnTransport, SummaryFault, fixtures_of, parse};
use crate::support::{Client, TestCore, kind, start_with_adapters, tempdir, workspace};
use ersa_adapter::Adapter;
use ersa_adapter::fake::FakeTransport;
use ersa_adapter::recording::Recording;
use ersa_adapter_claude::ClaudeAdapter;
use ersa_adapter_claude::record::encoded_cwd;
use serde_json::{Value, json};

/// The title the recorded session was given before its first message.
const NAMED: &str = "Named before the first message";

/// Fixture 09's stdout lines: `initialize`'s answer, the title the CLI
/// announced for the rename, the rename's answer, then the first turn.
fn recorded() -> Vec<String> {
    Recording::load(&fixtures_of("2.1.316").join("09-session-id-rename.jsonl"))
        .expect("fixture loads")
        .stdout()
}

/// The CLI's answer to the driver's first request after `initialize`, its
/// rename, which the driver numbers 3, after `get_context_usage`.
fn rename_answer() -> String {
    json!({"type": "control_response",
        "response": {"subtype": "success", "request_id": "ersa-rename-3"}})
    .to_string()
}

/// The CLI's announcement of `title` as the session's name.
fn announced(stdout: &[String], title: &str) -> String {
    let mut line = parse(&stdout[1]);
    assert_eq!(line["subtype"], "session_title_changed");
    line["title"] = json!(title);
    line.to_string()
}

/// A stub check that the stdin line is a `kind` control request or message.
pub(crate) fn is(
    kind: &'static str,
) -> impl Fn(&str) -> Result<(), String> + Send + Sync + 'static {
    move |line: &str| {
        let v = parse(line);
        let found = v["request"]["subtype"]
            .as_str()
            .or_else(|| v["type"].as_str())
            .unwrap_or_default()
            .to_string();
        if found == kind {
            Ok(())
        } else {
            Err(format!("expected {kind}, got {line}"))
        }
    }
}

/// A Core whose only adapter is Claude Code's, starting its CLI on
/// `transport`, with session records under `home`.
pub(crate) async fn claude_core(transport: &FakeTransport, home: &Path) -> TestCore {
    claude_core_with_fault(transport, home, Arc::default()).await
}

/// [`claude_core`], whose summary reads go wrong while `fault` says so.
async fn claude_core_with_fault(
    transport: &FakeTransport,
    home: &Path,
    fault: Arc<Mutex<Option<SummaryFault>>>,
) -> TestCore {
    let adapter = OnTransport {
        adapter: ClaudeAdapter::new()
            .with_claude_home(home.to_path_buf())
            .with_stop_grace(Duration::from_secs(1)),
        transport: Arc::new(transport.clone()),
        summary_fault: fault,
    };
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(adapter)];
    start_with_adapters(tempdir(), |_| {}, adapters).await
}

/// Creates a Claude Code session; returns it as `session.create` did.
pub(crate) async fn create(core: &TestCore, client: &mut Client) -> Value {
    let workspace_id = workspace(client, core.dir.path()).await;
    client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": "claude-code" }),
        )
        .await
}

/// The `--session-id` the transport's first CLI was started with.
fn chosen_id(transport: &FakeTransport) -> String {
    let args = transport.spawned()[0].args.clone();
    let at = args
        .iter()
        .position(|a| a == "--session-id")
        .unwrap_or_else(|| panic!("no --session-id in {args:?}"));
    args[at + 1].clone()
}

pub(crate) async fn listed_title(client: &mut Client, id: &str) -> Value {
    let list = client.result("session.list", json!({})).await;
    list.as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .expect("the session is listed")["title"]
        .clone()
}

async fn next_event(client: &mut Client, event_type: &str) -> Value {
    client.events_until(event_type).await.pop().unwrap()["event"].clone()
}

/// Fixture 09's record, filed as the CLI files session `session_id`'s
/// record for `cwd`.
fn write_record(home: &Path, cwd: &str, session_id: &str) -> PathBuf {
    let dir = home.join("projects").join(encoded_cwd(cwd));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{session_id}.jsonl"));
    std::fs::copy(
        fixtures_of("2.1.316").join("record/09-session-id-rename.jsonl"),
        &file,
    )
    .unwrap();
    file
}

/// Waits until the transport's first CLI has been sent `initialize`.
async fn initialize_sent(transport: &FakeTransport) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !transport
        .stdin_lines()
        .iter()
        .any(|l| is("initialize")(l).is_ok())
    {
        assert!(tokio::time::Instant::now() < deadline, "no initialize sent");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R35
// verifies: PROTO-CLAUDE-CODE-STREAM-R36
// verifies: PROTO-CORE-API-R51
#[tokio::test(flavor = "multi_thread")]
async fn a_starting_session_is_renamed_once_the_cli_answers_initialize() {
    let stdout = recorded();
    // The CLI takes a while to start, so the rename arrives while it does.
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(is("initialize"))
        .then_pause(Duration::from_millis(800))
        .then_stdout([&stdout[0]])
        .wait_for_stdin(is("get_context_usage"))
        .wait_for_stdin(is("rename_session"))
        .then_stdout([announced(&stdout, NAMED), rename_answer()])
        .wait_for_stdin(is("user"))
        .then_stdout(&stdout[3..])
        .hold_open();
    let home = tempdir();
    let core = claude_core(&transport, home.path()).await;
    let mut client = core.client().await;
    let session = create(&core, &mut client).await;
    let id = session["id"].as_str().unwrap().to_string();
    assert_eq!(session["state"], "starting");
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;

    // Not refused for starting: held until the CLI is ready, then sent, and
    // listed at once.
    let renamed = client
        .result("session.set_title", json!({ "id": id, "title": NAMED }))
        .await;
    assert_eq!(renamed["title"], NAMED);
    assert_eq!(listed_title(&mut client, &id).await, NAMED);

    // The CLI was started under the id Core chose, and the rename names that
    // id, which Core read from the session it stored.
    let session_id = chosen_id(&transport);
    let sent: Vec<Value> = transport.stdin_lines().iter().map(|l| parse(l)).collect();
    assert_eq!(sent.len(), 3, "{sent:#?}");
    assert_eq!(sent[0]["request"]["subtype"], "initialize");
    assert_eq!(sent[1]["request"]["subtype"], "get_context_usage");
    assert_eq!(
        sent[2]["request"],
        json!({ "subtype": "rename_session", "title": NAMED, "source": "host",
            "session_id": session_id })
    );
    // It went out only once `initialize` had its answer. The raw log records
    // a line as it crosses, which can trail the stub's copy of stdin, so
    // wait for both lines to be in it.
    let is_init = |l: &Value| l["response"]["request_id"] == "ersa-init-1";
    let is_rename = |l: &Value| l["request"]["subtype"] == "rename_session";
    let mut positions = None;
    let mut log = Value::Null;
    for _ in 0..200 {
        log = client.result("session.raw_log", json!({ "id": id })).await;
        let lines: Vec<Value> = log
            .as_array()
            .unwrap()
            .iter()
            .map(|l| parse(l["line"].as_str().unwrap()))
            .collect();
        if let (Some(i), Some(r)) = (
            lines.iter().position(is_init),
            lines.iter().position(is_rename),
        ) {
            positions = Some((i, r));
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let (initialized, rename_sent) =
        positions.unwrap_or_else(|| panic!("the raw log lacks a line: {log:#}"));
    assert!(initialized < rename_sent, "{log:#}");

    // The first message; the name stays.
    client
        .result(
            "session.send",
            json!({ "id": id, "text": "Reply with the single word: ok" }),
        )
        .await;
    next_event(&mut client, "turn.completed").await;
    assert_eq!(listed_title(&mut client, &id).await, NAMED);
    client.result("session.stop", json!({ "id": id })).await;
    assert!(
        transport.stub_failures().is_empty(),
        "{:?}",
        transport.stub_failures()
    );
    // The stub CLI writes no record, so the name it announced is still the
    // session's.
    assert_eq!(listed_title(&mut client, &id).await, NAMED);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R36
#[tokio::test(flavor = "multi_thread")]
async fn the_name_the_cli_announces_for_a_rename_is_the_one_shown() {
    let stdout = recorded();
    // The CLI made the name unique. Its announcement is projected well
    // before its answer reaches the rename.
    let unique = format!("{NAMED} (2)");
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(is("initialize"))
        .then_stdout([&stdout[0]])
        .wait_for_stdin(is("get_context_usage"))
        .wait_for_stdin(is("rename_session"))
        .then_stdout([announced(&stdout, &unique)])
        .then_pause(Duration::from_millis(300))
        .then_stdout([rename_answer()])
        // Asked for the same name again, it announces the same unique one.
        .wait_for_stdin(is("rename_session"))
        .then_stdout([announced(&stdout, &unique)])
        .then_pause(Duration::from_millis(300))
        .then_stdout([json!({"type": "control_response",
            "response": {"subtype": "success", "request_id": "ersa-rename-4"}})
        .to_string()])
        .hold_open();
    let home = tempdir();
    let core = claude_core(&transport, home.path()).await;
    let mut client = core.client().await;
    let id = create(&core, &mut client).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let renamed = client
        .result("session.set_title", json!({ "id": id, "title": NAMED }))
        .await;
    assert_eq!(renamed["title"], unique.as_str());
    assert_eq!(listed_title(&mut client, &id).await, unique.as_str());
    let renamed = client
        .result("session.set_title", json!({ "id": id, "title": NAMED }))
        .await;
    assert_eq!(renamed["title"], unique.as_str());
    assert_eq!(listed_title(&mut client, &id).await, unique.as_str());
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R36
#[tokio::test(flavor = "multi_thread")]
async fn a_title_the_cli_announces_is_the_sessions_until_its_record_holds_one() {
    let stdout = recorded();
    // Named in the session itself (a `/rename`, say): nothing from Ersa.
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(is("initialize"))
        .then_stdout([&stdout[0]])
        .wait_for_stdin(is("get_context_usage"))
        .wait_for_stdin(is("user"))
        .then_stdout([announced(&stdout, "Named in the session")])
        .then_stdout(&stdout[3..])
        .hold_open();
    let home = tempdir();
    let core = claude_core(&transport, home.path()).await;
    let mut client = core.client().await;
    let id = create(&core, &mut client).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(listed_title(&mut client, &id).await, Value::Null);
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    let turn = client.events_until("turn.completed").await;
    assert_eq!(listed_title(&mut client, &id).await, "Named in the session");

    // Once the record holds a title, the record's is the session's: Core
    // reads it once the session ends. The record is filed under the id the
    // CLI reported, which a real CLI takes from `--session-id` and this
    // recording took when it was recorded.
    let reported = turn
        .iter()
        .find(|e| e["event"]["type"] == "session.started")
        .expect("the CLI announced the session")["event"]["backend_session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let cwd = transport.spawned()[0].cwd.clone().unwrap();
    write_record(home.path(), &cwd, &reported);
    client.result("session.stop", json!({ "id": id })).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while listed_title(&mut client, &id).await != NAMED {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the record's title never showed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R35
#[tokio::test(flavor = "multi_thread")]
async fn a_session_stopped_before_its_first_message_keeps_its_id_and_has_no_record() {
    let stdout = recorded();
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(is("initialize"))
        .then_stdout([&stdout[0]])
        .hold_open();
    let home = tempdir();
    let core = claude_core(&transport, home.path()).await;
    let mut client = core.client().await;
    let id = create(&core, &mut client).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    initialize_sent(&transport).await;
    client.result("session.stop", json!({ "id": id })).await;

    // Its stored id names no record yet: an empty session, not one whose
    // history could not be read.
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let events = client.events_until("session.ended").await;
    assert!(
        events.iter().all(|e| e["event"]["type"] != "error"),
        "{events:#?}"
    );
    // It ended before the CLI recorded it, so there is nothing to rename.
    let error = client
        .error(
            "session.set_title",
            json!({ "id": id, "title": "Too late" }),
        )
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error:#}");

    // The id is the one the CLI was started under, kept with the session:
    // once a record exists under it, the session is renamed there.
    let cwd = transport.spawned()[0].cwd.clone().unwrap();
    let file = write_record(home.path(), &cwd, &chosen_id(&transport));
    let renamed = client
        .result("session.set_title", json!({ "id": id, "title": "Kept" }))
        .await;
    assert_eq!(renamed["title"], "Kept");
    let record = std::fs::read_to_string(&file).unwrap();
    let last = record.lines().last().unwrap();
    assert!(last.contains(r#""customTitle":"Kept""#), "{last}");

    // A record gone again: nothing to rename.
    std::fs::remove_file(&file).unwrap();
    let error = client
        .error("session.set_title", json!({ "id": id, "title": "Gone" }))
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error:#}");
}

// verifies: FLOW-FORK-SESSION-R01
// verifies: PROTO-CLAUDE-CODE-STREAM-R38
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_is_ready_before_its_first_message_and_shows_the_copied_conversation() {
    let stdout = recorded();
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(is("initialize"))
        // As the CLI does for `--name`: announced before it answers.
        .then_stdout([
            announced(&stdout, &format!("{NAMED} (fork)")),
            stdout[0].clone(),
        ])
        .hold_open();
    let home = tempdir();
    let core = claude_core(&transport, home.path()).await;
    let mut client = core.client().await;
    let original = create(&core, &mut client).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    initialize_sent(&transport).await;
    client
        .result("session.stop", json!({ "id": original }))
        .await;
    let original_backend_id = chosen_id(&transport);
    let cwd = transport.spawned()[0].cwd.clone().unwrap();
    let record = write_record(home.path(), &cwd, &original_backend_id);
    let before = std::fs::read(&record).unwrap();
    // Reading the stopped session takes its title from the record.
    client
        .result(
            "session.subscribe",
            json!({ "id": original, "from_seq": 0 }),
        )
        .await;
    client.events_until("session.ended").await;
    assert_eq!(listed_title(&mut client, &original).await, NAMED);

    let fork = client
        .result("session.fork", json!({ "id": original }))
        .await;
    let id = fork["id"].as_str().unwrap().to_string();
    // Core holds no title of its own for it.
    // verifies: FLOW-FORK-SESSION-R08
    assert_eq!(fork["title"], Value::Null);

    // A session of its own, from a copy of the original.
    // `session.fork` answers once the row exists; the CLI starts after it.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while transport.spawned().len() < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the fork's CLI never started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let spawned = transport.spawned()[1].clone();
    // The original runs in an isolated workspace, so the fork has its own.
    let fork_cwd = spawned.cwd.clone().expect("a working directory");
    assert_ne!(fork_cwd, cwd);
    assert!(Path::new(&fork_cwd).is_dir(), "{fork_cwd}");
    let at = |flag: &str| spawned.args.iter().position(|a| a == flag);
    let resume = at("--resume").expect("--resume");
    assert_eq!(spawned.args[resume + 1], original_backend_id);
    assert!(at("--fork-session").is_some(), "{:?}", spawned.args);
    // verifies: PROTO-CLAUDE-CODE-STREAM-R45
    let name = at("--name").expect("--name");
    assert_eq!(spawned.args[name + 1], format!("{NAMED} (fork)"));
    let chosen = spawned.args[at("--session-id").expect("--session-id") + 1].clone();
    assert_ne!(chosen, original_backend_id);

    // Ready for its first message, which the CLI will not report a session
    // before: no `starting` until then.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let list = client.result("session.list", json!({})).await;
        let row = list
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .cloned()
            .unwrap();
        if row["state"] == "idle" {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{row:#}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // The conversation shows from the original's record, and that record
    // is as it was.
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let events = client.events_until("assistant.text").await;
    assert!(
        events.iter().any(|e| e["event"]["type"] == "user.message"),
        "{events:#?}"
    );
    assert_eq!(std::fs::read(&record).unwrap(), before);

    // verifies: FLOW-FORK-SESSION-R08
    // The title it shows is the one the backend announced, with no rename.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while listed_title(&mut client, &id).await != format!("{NAMED} (fork)").as_str() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the announced title never showed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        !transport
            .stdin_lines()
            .iter()
            .any(|l| parse(l)["request"]["subtype"] == "rename_session"),
        "no rename is needed"
    );
}

/// The `backend_session_id` `session.list` carries for session `id`.
async fn listed_backend_id(client: &mut Client, id: &str) -> Value {
    let list = client.result("session.list", json!({})).await;
    list.as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .expect("the session is listed")["backend_session_id"]
        .clone()
}

/// The session id the recorded CLI announced in its `init` line: the one a
/// real CLI takes from `--session-id`, and this recording took when it was
/// recorded.
fn recorded_session_id(stdout: &[String]) -> String {
    stdout
        .iter()
        .map(|l| parse(l))
        .find(|v| v["type"] == "system" && v["subtype"] == "init")
        .expect("the recording has an init line")["session_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Waits for a `session.tile` for session `id` carrying `backend_session_id`.
async fn tile_carrying(client: &mut Client, id: &str, backend_id: &str) -> Value {
    let wanted = |n: &Value| {
        n["method"] == "session.tile"
            && n["params"]["session_id"] == id
            && n["params"]["backend_session_id"] == backend_id
    };
    tokio::time::timeout(Duration::from_secs(10), client.notification_where(wanted))
        .await
        .expect("no session.tile carried the backend's id")
}

/// Waits for the turn's end and a `session.tile` for session `id` carrying
/// `backend_id`, in either order. Core publishes the end to the stream and
/// then looks for the record apart from it, so on a busy machine the look's
/// tile can reach the client first; waiting for the end alone would read
/// past the tile and drop it.
async fn turn_end_and_tile_carrying(client: &mut Client, id: &str, backend_id: &str) {
    let (mut ended, mut carried) = (false, false);
    let both = async {
        while !(ended && carried) {
            let n = client
                .notification()
                .await
                .expect("the connection stays open");
            ended |= n["method"] == "session.event"
                && n["params"]["event"]["event"]["type"] == "turn.completed";
            carried |= n["method"] == "session.tile"
                && n["params"]["session_id"] == id
                && n["params"]["backend_session_id"] == backend_id;
        }
    };
    let finished = tokio::time::timeout(Duration::from_secs(10), both)
        .await
        .is_ok();
    assert!(
        finished,
        "within 10 seconds: turn ended {ended}, a tile carried the backend's id {carried}"
    );
}

/// Claude Code starts a session under an id Core chose, but has a record
/// under it only from the session's first message: until then the id finds
/// nothing in its own tools, so Core carries none, in the answer or the list.
/// A turn's end looks for the record, and a tile then carries the id before
/// any list is read.
// verifies: FLOW-RESUME-SESSION-R18
// verifies: FLOW-RESUME-SESSION-R19
// verifies: FLOW-RESUME-SESSION-R20
#[tokio::test(flavor = "multi_thread")]
async fn a_session_carries_the_id_core_chose_once_a_turn_ends_with_its_record_written() {
    let stdout = recorded();
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(is("initialize"))
        .then_stdout([&stdout[0]])
        .wait_for_stdin(is("get_context_usage"))
        .wait_for_stdin(is("user"))
        .then_stdout(&stdout[3..])
        .hold_open();
    let home = tempdir();
    let core = claude_core(&transport, home.path()).await;
    let mut client = core.client().await;
    let created = create(&core, &mut client).await;
    let id = created["id"].as_str().unwrap().to_string();
    initialize_sent(&transport).await;
    // Started under an id Core chose, with no record under it yet.
    let chosen = chosen_id(&transport);
    assert!(created.get("backend_session_id").is_none(), "{created:#}");
    assert_eq!(listed_backend_id(&mut client, &id).await, Value::Null);

    // The CLI writes the record with the first message. The stub writes
    // none, so the test files it as the CLI would, under the id the
    // recording announces (a real CLI's is `--session-id`'s).
    let named = recorded_session_id(&stdout);
    assert_ne!(named, chosen);
    let cwd = transport.spawned()[0].cwd.clone().unwrap();
    let record = write_record(home.path(), &cwd, &named);
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    // The turn's end looked, and a tile carries the id with no list read.
    turn_end_and_tile_carrying(&mut client, &id, &named).await;
    assert_eq!(listed_backend_id(&mut client, &id).await, json!(named));

    // Still running, its record gone: the next list's look finds nothing,
    // so the id leads nowhere and is not carried.
    std::fs::remove_file(&record).unwrap();
    assert_eq!(listed_backend_id(&mut client, &id).await, Value::Null);
}

/// Once a look found the session's record, a look that fails, and one that
/// runs out of time, are no answer: the id stays carried, in the list, and
/// no tile goes out saying otherwise.
// verifies: FLOW-RESUME-SESSION-R22
#[tokio::test(flavor = "multi_thread")]
async fn a_look_that_fails_or_runs_out_of_time_leaves_the_id_carried() {
    let stdout = recorded();
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(is("initialize"))
        .then_stdout([&stdout[0]])
        .wait_for_stdin(is("get_context_usage"))
        .wait_for_stdin(is("user"))
        .then_stdout(&stdout[3..])
        .hold_open();
    let home = tempdir();
    let fault: Arc<Mutex<Option<SummaryFault>>> = Arc::default();
    let core = claude_core_with_fault(&transport, home.path(), Arc::clone(&fault)).await;
    let mut client = core.client().await;
    let id = create(&core, &mut client).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    initialize_sent(&transport).await;
    let named = recorded_session_id(&stdout);
    let cwd = transport.spawned()[0].cwd.clone().unwrap();
    write_record(home.path(), &cwd, &named);
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    turn_end_and_tile_carrying(&mut client, &id, &named).await;
    assert_eq!(listed_backend_id(&mut client, &id).await, json!(named));

    for kind in [SummaryFault::Fails, SummaryFault::Hangs] {
        *fault.lock().unwrap() = Some(kind);
        client.clear_notifications();
        // The list waits for its looks, the one that hangs up to its limit,
        // so any tile they sent is in before its answer.
        assert_eq!(
            listed_backend_id(&mut client, &id).await,
            json!(named),
            "{kind:?}"
        );
        let tile_for = |n: &Value| n["method"] == "session.tile" && n["params"]["session_id"] == id;
        let tile = tokio::time::timeout(
            Duration::from_millis(200),
            client.notification_where(tile_for),
        )
        .await;
        assert!(tile.is_err(), "{kind:?} sent {tile:?}");
    }
}

/// A fork, also started under an id Core chose, shows its original's
/// conversation but has no record of its own until its first message: Core
/// carries no id for it, in `session.fork`'s answer or the list, until a
/// list's look finds the fork's own record, and a tile then carries it too.
// verifies: FLOW-RESUME-SESSION-R18
// verifies: FLOW-RESUME-SESSION-R19
// verifies: FLOW-RESUME-SESSION-R20
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_carries_the_id_core_chose_once_a_list_finds_its_own_record() {
    let stdout = recorded();
    // Only `initialize` is answered: the stub's stdin waits count every
    // process's lines, so a longer script would replay into the fork.
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(is("initialize"))
        .then_stdout([&stdout[0]])
        .hold_open();
    let home = tempdir();
    let core = claude_core(&transport, home.path()).await;
    let mut client = core.client().await;
    let id = create(&core, &mut client).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    initialize_sent(&transport).await;
    let chosen = chosen_id(&transport);

    // A fork needs its original's record to copy; the original's own look
    // finds it once the session is listed.
    client.result("session.stop", json!({ "id": id })).await;
    let cwd = transport.spawned()[0].cwd.clone().unwrap();
    write_record(home.path(), &cwd, &chosen);
    assert_eq!(listed_backend_id(&mut client, &id).await, json!(chosen));

    let fork = client.result("session.fork", json!({ "id": id })).await;
    let fork_id = fork["id"].as_str().unwrap().to_string();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while transport.spawned().len() < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the fork's CLI never started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let args = transport.spawned()[1].args.clone();
    let at = args
        .iter()
        .position(|a| a == "--session-id")
        .expect("--session-id");
    let fork_chosen = args[at + 1].clone();
    assert_ne!(fork_chosen, chosen);
    assert!(fork.get("backend_session_id").is_none(), "{fork:#}");
    assert_eq!(listed_backend_id(&mut client, &fork_id).await, Value::Null);
    // The original keeps its own.
    assert_eq!(listed_backend_id(&mut client, &id).await, json!(chosen));

    // Once the backend has the fork's record, the list's look finds it.
    write_record(home.path(), &cwd, &fork_chosen);
    assert_eq!(
        listed_backend_id(&mut client, &fork_id).await,
        json!(fork_chosen)
    );
    tile_carrying(&mut client, &fork_id, &fork_chosen).await;
}
