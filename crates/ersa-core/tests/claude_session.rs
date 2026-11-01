//! A Claude Code session driven through Core's API over the real socket,
//! with the real Claude adapter talking to a stub CLI that replays recorded
//! fixture 11: a prompt, a permission request, the user's allow, the reply.

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, Projector, SessionConfig, Transport,
    async_trait,
};
use ersa_adapter_claude::ClaudeAdapter;
use serde_json::{Value, json};
use support::{Client, start_with_adapters, tempdir, workspace};

const FIXTURE: &str = "11-stream-permission-prompt-tool-stdio";

/// The Claude adapter, made to start its CLI on a given transport instead of
/// the one Core hands it.
struct OnTransport {
    adapter: ClaudeAdapter,
    transport: Arc<dyn Transport>,
}

#[async_trait]
impl Adapter for OnTransport {
    fn backend(&self) -> &str {
        self.adapter.backend()
    }

    async fn start(
        &self,
        _: Arc<dyn Transport>,
        config: SessionConfig,
        sink: Arc<dyn AdapterSink>,
    ) -> Result<Box<dyn AdapterSession>, AdapterError> {
        self.adapter
            .start(Arc::clone(&self.transport), config, sink)
            .await
    }

    fn projector(&self) -> Box<dyn Projector> {
        self.adapter.projector()
    }
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("docs/briefs/protocol/fixtures/claude-code/2.1.302")
}

/// The fixture's stdout lines, and the lines the recording wrote to stdin.
fn fixture() -> (Vec<String>, Vec<String>) {
    let dir = fixture_dir();
    let stdout = std::fs::read_to_string(dir.join(format!("{FIXTURE}.jsonl"))).unwrap();
    let stdout: Vec<String> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect();
    let meta: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{FIXTURE}.meta.json"))).unwrap())
            .unwrap();
    let sent = meta["sent"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["line"].as_str().unwrap().to_string())
        .collect();
    (stdout, sent)
}

fn parse(line: &str) -> Value {
    serde_json::from_str(line).unwrap()
}

/// A stub CLI that answers each of Ersa's lines with the recording's next
/// stdout lines: `initialize`, then the prompt, then the permission answer.
fn stub(stdout: &[String]) -> FakeTransport {
    let is = |kind: &'static str| {
        move |line: &str| {
            let v = parse(line);
            let ok = match kind {
                "initialize" => v["request"]["subtype"] == "initialize",
                "user" => v["type"] == "user",
                _ => v["type"] == "control_response",
            };
            if ok { Ok(()) } else { Err(format!("expected {kind}, got {line}")) }
        }
    };
    FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(is("initialize"))
        .then_stdout([&stdout[0]])
        .wait_for_stdin(is("user"))
        .then_stdout(&stdout[1..=4])
        .wait_for_stdin(is("answer"))
        .then_stdout(&stdout[5..])
        .hold_open()
}

async fn next_event(client: &mut Client, event_type: &str) -> Value {
    client.events_until(event_type).await.pop().unwrap()["event"].clone()
}

// verifies: PROTO-CORE-API-R04
// verifies: PROTO-CORE-API-R08
#[tokio::test(flavor = "multi_thread")]
async fn a_claude_session_round_trips_a_permission_through_core() {
    let (stdout, sent) = fixture();
    let transport = stub(&stdout);
    let adapter = OnTransport {
        adapter: ClaudeAdapter::new().with_stop_grace(Duration::from_secs(1)),
        transport: Arc::new(transport.clone()),
    };
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(adapter)];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let mut client = core.client().await;
    let workspace_id = workspace(&mut client, core.dir.path()).await;

    let session = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": "claude-code" }),
        )
        .await;
    let id = session["id"].as_str().unwrap().to_string();
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    // The prompt the recording sent, so the stub's lines answer it.
    let prompt = parse(&sent[1])["message"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    let turn = client
        .result("session.send", json!({ "id": id, "text": prompt }))
        .await;
    assert_eq!(turn["turn"], 1);
    // The CLI announces itself once it has the first prompt.
    let started = next_event(&mut client, "session.started").await;
    assert_eq!(started["backend_version"], "2.1.302");

    let asked = next_event(&mut client, "permission.requested").await;
    let request_id = asked["request_id"].as_str().unwrap().to_string();
    assert_eq!(asked["tool_name"], "Bash");
    let row = client.result("session.list", json!({})).await[0].clone();
    assert_eq!(
        row["tile"]["pending"],
        json!([{ "kind": "permission", "request_id": request_id }])
    );
    assert_eq!(row["state"], "waiting_user");

    client
        .result(
            "session.decide_permission",
            json!({ "id": id, "request_id": request_id, "behavior": "allow" }),
        )
        .await;
    let decided = next_event(&mut client, "permission.decided").await;
    assert_eq!(decided["request_id"], request_id.as_str());
    assert_eq!(decided["behavior"], "allow");
    let completed = next_event(&mut client, "turn.completed").await;
    assert_eq!(completed["outcome"], "success");

    let stopped = client.result("session.stop", json!({ "id": id })).await;
    assert_eq!(stopped["state"], "ended");
    let ended = next_event(&mut client, "session.ended").await;
    assert_eq!(ended["reason"], "user_stopped");
    assert_eq!(stopped["tile"]["pending"], json!([]));
    assert!(stopped["tile"]["cost_usd_total"].as_f64().unwrap() > 0.0);

    assert_eq!(transport.stub_failures(), Vec::<String>::new());
    // Ersa wrote exactly what the recorded session wrote.
    assert_eq!(transport.stdin_lines(), sent);
    // And the raw log holds both directions exactly as they crossed, between
    // Core's own records.
    let log = client.result("session.raw_log", json!({ "id": id })).await;
    let log = log.as_array().unwrap();
    let crossed: Vec<(String, String)> = log
        .iter()
        .filter(|e| e["dir"] != "core")
        .map(|e| (e["dir"].as_str().unwrap().into(), e["line"].as_str().unwrap().into()))
        .collect();
    let backend: Vec<&String> = crossed
        .iter()
        .filter(|(dir, _)| dir == "backend")
        .map(|(_, line)| line)
        .collect();
    let host: Vec<&String> = crossed
        .iter()
        .filter(|(dir, _)| dir == "host")
        .map(|(_, line)| line)
        .collect();
    assert_eq!(backend, stdout.iter().collect::<Vec<_>>());
    assert_eq!(host, sent.iter().collect::<Vec<_>>());
    // Every answer after the line that asked for it.
    let position = |line: &String| crossed.iter().position(|(_, l)| l == line).unwrap();
    assert!(position(&sent[0]) < position(&stdout[0]));
    assert!(position(&stdout[4]) < position(&sent[2]));
    assert_eq!(log.first().unwrap()["dir"], "core");
    assert_eq!(log.last().unwrap()["dir"], "core");
}
