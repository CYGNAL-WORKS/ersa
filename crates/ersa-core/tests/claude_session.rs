//! A Claude Code session driven through Core's API over the real socket,
//! with the real Claude adapter talking to a stub CLI that replays recorded
//! fixture 11: a prompt, a permission request, the user's allow, the reply.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::Adapter;
use serde_json::{Value, json};
use support::claude::{OnTransport, fixture, isolated_claude, parse, stub};
use support::{Client, start_with_adapters, tempdir, workspace};

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
        adapter: isolated_claude().with_stop_grace(Duration::from_secs(1)),
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
    // And the raw log holds both directions exactly as they crossed, and
    // nothing else: Core's lifecycle facts are in the app log.
    let log = client.result("session.raw_log", json!({ "id": id })).await;
    let log = log.as_array().unwrap();
    let crossed: Vec<(String, String)> = log
        .iter()
        .map(|e| {
            (
                e["dir"].as_str().unwrap().into(),
                e["line"].as_str().unwrap().into(),
            )
        })
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
    assert_eq!(crossed.len(), stdout.len() + sent.len());
}

/// The `--setting-sources` value of the `n`th CLI the transport started,
/// once it has started that many.
async fn setting_sources(transport: &ersa_adapter::fake::FakeTransport, n: usize) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(spec) = transport.spawned().get(n) {
            let at = spec
                .args
                .iter()
                .position(|a| a == "--setting-sources")
                .expect("--setting-sources passed");
            return spec.args[at + 1].clone();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no CLI number {n} started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R12
// verifies: PROTO-CLAUDE-CODE-STREAM-R18
#[tokio::test(flavor = "multi_thread")]
async fn a_projects_own_configuration_loads_only_once_the_user_trusts_it() {
    let (stdout, _) = fixture();
    let transport = stub(&stdout);
    let adapter = OnTransport {
        adapter: isolated_claude().with_stop_grace(Duration::from_secs(1)),
        transport: Arc::new(transport.clone()),
    };
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(adapter)];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let mut client = core.client().await;
    let repo = core.dir.path().join("repo");
    support::git_repo(&repo);
    let project = client
        .result(
            "project.add",
            json!({ "host_id": "local", "path": repo.display().to_string(), "trusted": false }),
        )
        .await;

    let start_session = async |client: &mut Client| {
        let workspace = client
            .result("workspace.create", json!({ "project_id": project["id"] }))
            .await;
        client
            .result(
                "session.create",
                json!({ "workspace_id": workspace["id"], "backend": "claude-code" }),
            )
            .await
    };

    start_session(&mut client).await;
    assert_eq!(setting_sources(&transport, 0).await, "user");

    client
        .result(
            "project.set_trust",
            json!({ "id": project["id"], "trusted": true }),
        )
        .await;
    start_session(&mut client).await;
    assert_eq!(setting_sources(&transport, 1).await, "user,project,local");
}
