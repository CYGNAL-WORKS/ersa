//! The real Claude adapter against a stub CLI that replays recorded fixture
//! 11: a prompt, a permission request, the user's allow, the reply.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, Projector, SessionConfig, Transport,
    async_trait,
};
use ersa_adapter_claude::ClaudeAdapter;
use serde_json::Value;

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

/// The fixture's stdout lines, and the lines the recording wrote to stdin.
pub fn fixture() -> (Vec<String>, Vec<String>) {
    let dir = fixture_dir();
    let stdout = std::fs::read_to_string(dir.join(format!("{FIXTURE}.jsonl"))).unwrap();
    let stdout: Vec<String> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect();
    let meta: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(format!("{FIXTURE}.meta.json"))).unwrap(),
    )
    .unwrap();
    let sent = meta["sent"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["line"].as_str().unwrap().to_string())
        .collect();
    (stdout, sent)
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
