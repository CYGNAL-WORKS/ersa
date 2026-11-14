//! A Claude Code session resumed while its transcript is open, through
//! Core's API with the real Claude adapter, over the recorded pair
//! 2.1.316/28-resume-original (the record the transcript was read from) and
//! 29-resume-while-open (the CLI's stream once resumed): the transcript
//! shows the record, then the new run, each message once
//! (FLOW-RESUME-SESSION-R01, PROTO-AGENT-EVENTS-R12).

use std::collections::VecDeque;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::claude::{OnTransport, fixtures_of, parse};
use crate::support::{start_with_adapters, tempdir, workspace};
use ersa_adapter::fake::FakeTransport;
use ersa_adapter::recording::Recording;
use ersa_adapter::{Adapter, Process, ProcessSpec, Transport, async_trait};
use ersa_adapter_claude::ClaudeAdapter;
use ersa_adapter_claude::record::encoded_cwd;
use serde_json::{Value, json};

/// The id the recorded session ran under.
const RECORDED_ID: &str = "c40e1ae0-6180-4454-acae-6cbc509f659b";

/// Hands each process it starts to the next stub in line, so one session's
/// runs each get a CLI of their own.
#[derive(Default)]
struct Runs(Mutex<VecDeque<FakeTransport>>);

impl Runs {
    fn then(&self, run: FakeTransport) {
        self.0.lock().unwrap().push_back(run);
    }
}

#[async_trait]
impl Transport for Runs {
    async fn spawn(&self, spec: ProcessSpec) -> io::Result<Box<dyn Process>> {
        let run = self.0.lock().unwrap().pop_front();
        match run {
            Some(run) => run.spawn(spec).await,
            None => Err(io::Error::other("no run left")),
        }
    }
}

/// Whether a stdin line is a `kind` control request or message.
fn is(kind: &'static str) -> impl Fn(&str) -> Result<(), String> + Send + Sync + 'static {
    move |line: &str| {
        let v = parse(line);
        let found = v["request"]["subtype"]
            .as_str()
            .or_else(|| v["type"].as_str());
        if found == Some(kind) {
            Ok(())
        } else {
            Err(format!("expected {kind}, got {line}"))
        }
    }
}

/// A fixture's stdout lines, under session id `id` instead of the recorded
/// one.
fn stdout_as(fixture: &str, id: &str) -> Vec<String> {
    Recording::load(&fixtures_of("2.1.316").join(format!("{fixture}.jsonl")))
        .expect("fixture loads")
        .stdout()
        .into_iter()
        .map(|line| line.replace(RECORDED_ID, id))
        .collect()
}

/// Every message's final text in `events`, with its kind and input.
fn messages(events: &[Value]) -> Vec<(String, String, String)> {
    events
        .iter()
        .filter(|e| match e["event"]["type"].as_str() {
            Some("user.message") => true,
            Some("assistant.text") => e["event"]["final"] == true,
            _ => false,
        })
        .map(|e| {
            let text = |v: &Value| v.as_str().unwrap_or_default().trim().to_string();
            (
                e["event"]["type"].as_str().unwrap().to_string(),
                e["source"]["input"].as_str().unwrap().to_string(),
                text(&e["event"]["text"]),
            )
        })
        .collect()
}

async fn wait_for_spawns(runs: &FakeTransport, n: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while runs.spawned().len() < n {
        assert!(tokio::time::Instant::now() < deadline, "no CLI started");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// verifies: FLOW-RESUME-SESSION-R01
#[tokio::test(flavor = "multi_thread")]
async fn a_session_resumed_while_open_shows_the_record_then_the_new_run_each_message_once() {
    // The first run: a new session, stopped before its first message here,
    // whose turn the recording took (fixture 28's record stands for it).
    let first = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(is("initialize"))
        .then_stdout(
            stdout_as("28-resume-original", RECORDED_ID)
                .into_iter()
                .take(1),
        )
        .hold_open();
    let runs = Arc::new(Runs::default());
    runs.then(first.clone());
    let home = tempdir();
    let adapter = OnTransport {
        adapter: ClaudeAdapter::new()
            .with_claude_home(home.path().to_path_buf())
            .with_stop_grace(Duration::from_secs(1)),
        transport: Arc::clone(&runs) as Arc<dyn Transport>,
    };
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(adapter)];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let mut client = core.client().await;
    let workspace_id = workspace(&mut client, core.dir.path()).await;
    let id = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": "claude-code" }),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    wait_for_spawns(&first, 1).await;
    client.result("session.stop", json!({ "id": id })).await;
    let spawned = first.spawned()[0].clone();
    let at = |args: &[String], flag: &str| args.iter().position(|a| a == flag);
    let chosen = spawned.args[at(&spawned.args, "--session-id").unwrap() + 1].clone();
    let cwd = spawned.cwd.clone().unwrap();

    // What the CLI kept of that turn, under the session's id.
    let dir = home.path().join("projects").join(encoded_cwd(&cwd));
    std::fs::create_dir_all(&dir).unwrap();
    let record =
        std::fs::read_to_string(fixtures_of("2.1.316").join("record/28-resume-original.jsonl"))
            .unwrap();
    std::fs::write(
        Path::new(&dir).join(format!("{chosen}.jsonl")),
        record.replace(RECORDED_ID, &chosen),
    )
    .unwrap();

    // The second run: the CLI resumed, as recorded.
    let resumed = stdout_as("29-resume-while-open", &chosen);
    let second = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(is("initialize"))
        .then_stdout(&resumed[..1])
        .wait_for_stdin(is("user"))
        .then_stdout(&resumed[1..])
        .hold_open();
    runs.then(second.clone());

    // The transcript is open on the record when the user writes again.
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let before = client.events_until("session.ended").await;
    client
        .result(
            "session.send",
            json!({ "id": id, "text": "Reply with only the word two." }),
        )
        .await;
    let mut after = Vec::new();
    while !messages(&after)
        .iter()
        .any(|(kind, _, text)| kind == "assistant.text" && text == "two")
    {
        after.extend(client.events_until("assistant.text").await);
    }

    // Resumed by the session's own id, in its own folder.
    let args = second.spawned()[0].args.clone();
    assert_eq!(args[at(&args, "--resume").expect("--resume") + 1], chosen);
    assert_eq!(at(&args, "--session-id"), None, "{args:?}");
    assert_eq!(second.spawned()[0].cwd.as_deref(), Some(cwd.as_str()));

    let all: Vec<Value> = before.iter().chain(&after).cloned().collect();
    let seqs: Vec<u64> = all.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, (0..all.len() as u64).collect::<Vec<_>>());
    let row = |kind: &str, input: &str, text: &str| {
        (kind.to_string(), input.to_string(), text.to_string())
    };
    assert_eq!(
        messages(&all),
        [
            row("user.message", "record", "Reply with only the word one."),
            row("assistant.text", "record", "one"),
            row("user.message", "live", "Reply with only the word two."),
            row("assistant.text", "live", "two"),
        ]
    );

    // The new run ends with a `session.ended` of its own.
    client.result("session.stop", json!({ "id": id })).await;
    let ended = client.events_until("session.ended").await;
    assert_eq!(ended.last().unwrap()["event"]["reason"], "user_stopped");
}
