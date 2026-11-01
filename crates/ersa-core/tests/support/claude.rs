//! The real Claude adapter against a stub CLI that replays recorded fixture
//! 11: a prompt, a permission request, the user's allow, the reply.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ersa_adapter::fake::FakeTransport;
use ersa_adapter::recording::Recording;
use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, Projector, SessionConfig, Transport,
    async_trait,
};
use ersa_adapter_claude::ClaudeAdapter;
use ersa_core::replay::{Pace, Replay};
use ersa_core::{CoreConfig, Server};
use serde_json::Value;

use super::{TestCore, tempdir, test_config, unique_endpoint};

pub const FIXTURE: &str = "11-stream-permission-prompt-tool-stdio";

/// The Claude adapter, made to start its CLI on a given transport instead of
/// the one Core hands it.
pub struct OnTransport {
    pub adapter: ClaudeAdapter,
    pub transport: Arc<dyn Transport>,
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

pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("docs/briefs/protocol/fixtures/claude-code/2.1.302")
}

/// Fixture 11 as a recording.
pub fn recording() -> Recording {
    Recording::load(&fixture_dir().join(format!("{FIXTURE}.jsonl"))).expect("fixture loads")
}

/// The fixture's stdout lines, and the lines the recording wrote to stdin.
pub fn fixture() -> (Vec<String>, Vec<String>) {
    let recording = recording();
    (recording.stdout(), recording.sent())
}

/// A replay server for fixture 11 in a temporary data directory, with
/// `configure` applied to its configuration.
pub async fn start_replay_with(configure: impl FnOnce(&mut CoreConfig), pace: Pace) -> TestCore {
    let dir = tempdir();
    let endpoint = unique_endpoint();
    let mut config = test_config(&dir.path().join("data"), &endpoint);
    configure(&mut config);
    let replay = Replay {
        adapter: Arc::new(ClaudeAdapter::new()),
        recording: recording(),
        title: FIXTURE.to_string(),
        // What detection finds in these tests' Cores, which have no backend
        // installed; a live test Core starts its sessions with it too.
        backend_version: String::new(),
        pace,
    };
    let server = Server::bind_replay(config, replay).await.expect("bind");
    let task = tokio::spawn(server.run());
    TestCore {
        dir,
        endpoint,
        task,
    }
}

pub fn parse(line: &str) -> Value {
    serde_json::from_str(line).unwrap()
}

/// A stub CLI that answers each of Ersa's lines with the recording's next
/// stdout lines: `initialize`, then the prompt, then the permission answer.
pub fn stub(stdout: &[String]) -> FakeTransport {
    let is = |kind: &'static str| {
        move |line: &str| {
            let v = parse(line);
            let ok = match kind {
                "initialize" => v["request"]["subtype"] == "initialize",
                "user" => v["type"] == "user",
                _ => v["type"] == "control_response",
            };
            if ok {
                Ok(())
            } else {
                Err(format!("expected {kind}, got {line}"))
            }
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
