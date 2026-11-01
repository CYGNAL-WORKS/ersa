//! A smoke test of the whole Core process against the real, installed
//! Claude Code CLI: `ersa-core serve`, create a session, send one prompt,
//! approve one permission, and watch the reply stream back.
//!
//! Ignored by default because it costs a model call and needs a logged-in
//! CLI. Run it by hand:
//!
//! ```text
//! cargo test -p ersa-cli --test live -- --ignored --nocapture
//! ```
//!
//! Core finds the CLI as the adapter does: `ERSA_CLAUDE`, else `claude` on
//! `PATH`. `ANTHROPIC_MODEL=haiku` in the environment keeps it cheap.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines, ReadHalf, WriteHalf};

const TURN: Duration = Duration::from_secs(180);

type Stream = ersa_core::platform::BoxStream;

struct Client {
    lines: Lines<BufReader<ReadHalf<Stream>>>,
    write: WriteHalf<Stream>,
    next_id: i64,
    notifications: Vec<Value>,
}

impl Client {
    async fn line(&mut self) -> Value {
        let line = tokio::time::timeout(TURN, self.lines.next_line())
            .await
            .expect("Core wrote in time")
            .expect("read")
            .expect("Core keeps the connection open");
        serde_json::from_str(&line).expect("JSON")
    }

    async fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let mut bytes = request.to_string().into_bytes();
        bytes.push(b'\n');
        self.write.write_all(&bytes).await.expect("write");
        loop {
            let line = self.line().await;
            if line.get("id").is_none() {
                self.notifications.push(line);
                continue;
            }
            match line.get("result") {
                Some(result) => return result.clone(),
                None => panic!("{method} failed: {line:#}"),
            }
        }
    }

    /// The next `session.event`'s event.
    async fn event(&mut self) -> Value {
        loop {
            let notification = if self.notifications.is_empty() {
                self.line().await
            } else {
                self.notifications.remove(0)
            };
            if notification["method"] == "session.event" {
                return notification["params"]["event"]["event"].clone();
            }
        }
    }
}

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Ersa",
            "-c",
            "user.email=tests@ersa.invalid",
        ])
        .args(args)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?}");
}

#[tokio::test]
#[ignore = "drives the installed Claude Code CLI with the user's login; run by hand"]
async fn a_prompt_with_a_permission_round_trip_through_the_core_process() {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(repo.join("README.md"), "smoke test\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "--quiet", "-m", "initial"]);

    let endpoint = format!(
        "{}-live-{}",
        ersa_core::platform::default_endpoint().unwrap(),
        std::process::id()
    );
    let mut core = tokio::process::Command::new(env!("CARGO_BIN_EXE_ersa-core"))
        .args(["serve", "--endpoint", &endpoint, "--data-dir"])
        .arg(dir.path().join("data"))
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("ersa-core starts");

    let mut stream = None;
    for _ in 0..100 {
        if let Ok(connected) = ersa_core::connect(&endpoint).await {
            stream = Some(connected);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let (read, write) = tokio::io::split(stream.expect("Core is listening"));
    let mut client = Client {
        lines: BufReader::new(read).lines(),
        write,
        next_id: 1,
        notifications: Vec::new(),
    };
    client
        .call(
            "core.hello",
            json!({ "client": "live-test", "client_version": "0", "api_versions": [1] }),
        )
        .await;
    let project = client
        .call(
            "project.add",
            json!({ "host_id": "local", "path": repo.display().to_string() }),
        )
        .await;
    let workspace = client
        .call("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    let session = client
        .call(
            "session.create",
            json!({ "workspace_id": workspace["id"], "backend": "claude-code" }),
        )
        .await;
    let id = session["id"].clone();
    client
        .call("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .call(
            "session.send",
            json!({
                "id": id,
                "text": "Use the Bash tool to run exactly this command: echo probe > probe.txt. \
                         Then reply with the single word done."
            }),
        )
        .await;

    let mut texts = Vec::new();
    let mut decided = false;
    loop {
        let event = client.event().await;
        match event["type"].as_str().unwrap_or_default() {
            "permission.requested" => {
                client
                    .call(
                        "session.decide_permission",
                        json!({ "id": id, "request_id": event["request_id"], "behavior": "allow" }),
                    )
                    .await;
            }
            "permission.decided" => decided = true,
            "assistant.text" => texts.push(event["text"].clone()),
            "turn.completed" => {
                assert_eq!(event["outcome"], "success", "{event:#}");
                break;
            }
            "session.ended" => panic!("the session ended early: {event:#}"),
            _ => {}
        }
    }
    assert!(decided, "a permission request round-tripped");
    assert!(!texts.is_empty(), "the reply streamed back");
    println!("reply: {texts:?}");

    client.call("session.stop", json!({ "id": id })).await;
    client.call("core.shutdown", json!({})).await;
    let status = tokio::time::timeout(Duration::from_secs(30), core.wait())
        .await
        .expect("Core exited")
        .expect("wait");
    assert!(status.success());
}
