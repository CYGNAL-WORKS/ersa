//! The real Claude adapter against a stub CLI that replays recorded fixture
//! 11: a prompt, a permission request, the user's allow, the reply.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ersa_adapter::fake::FakeTransport;
use ersa_adapter::recording::Recording;
use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, OpenElsewhere, Projector, RecordEvent,
    RecordSummary, SessionConfig, Transport, async_trait,
};
use ersa_adapter_claude::ClaudeAdapter;
use ersa_core::CoreConfig;
use ersa_core::replay::{Pace, Replay};
use serde_json::Value;

use crate::support::{TestCore, start_replay_of};

pub const FIXTURE: &str = "11-stream-permission-prompt-tool-stdio";

/// The Claude adapter, made to start its CLI on a given transport instead of
/// the one Core hands it. Everything else is the adapter's own, its session
/// records read from its Claude home.
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

    fn set_environment(&self, spec: &mut ersa_adapter::ProcessSpec) {
        self.adapter.set_environment(spec)
    }

    fn tested_versions(&self) -> Option<(&'static str, &'static str)> {
        self.adapter.tested_versions()
    }

    fn title_rules(&self) -> ersa_adapter::TitleRules {
        self.adapter.title_rules()
    }

    fn projector(&self) -> Box<dyn Projector> {
        self.adapter.projector()
    }

    fn new_session_id(&self) -> Option<String> {
        self.adapter.new_session_id()
    }

    async fn read_record(
        &self,
        transport: Arc<dyn Transport>,
        cwd: &str,
        backend_session_id: &str,
    ) -> Result<Arc<Vec<RecordEvent>>, AdapterError> {
        self.adapter
            .read_record(transport, cwd, backend_session_id)
            .await
    }

    async fn record_version(
        &self,
        transport: Arc<dyn Transport>,
        cwd: &str,
        backend_session_id: &str,
    ) -> Result<Option<String>, AdapterError> {
        self.adapter
            .record_version(transport, cwd, backend_session_id)
            .await
    }

    fn forget_record(&self, cwd: &str, backend_session_id: &str) {
        self.adapter.forget_record(cwd, backend_session_id)
    }

    async fn summarize_record(
        &self,
        transport: Arc<dyn Transport>,
        cwd: &str,
        backend_session_id: &str,
    ) -> Result<Option<RecordSummary>, AdapterError> {
        self.adapter
            .summarize_record(transport, cwd, backend_session_id)
            .await
    }

    async fn rename_record(
        &self,
        transport: Arc<dyn Transport>,
        cwd: &str,
        backend_session_id: &str,
        title: &str,
    ) -> Result<(), AdapterError> {
        self.adapter
            .rename_record(transport, cwd, backend_session_id, title)
            .await
    }

    async fn conversation_end(
        &self,
        transport: Arc<dyn Transport>,
        cwd: &str,
        backend_session_id: &str,
    ) -> Result<Option<ersa_adapter::ConversationEnd>, AdapterError> {
        self.adapter
            .conversation_end(transport, cwd, backend_session_id)
            .await
    }

    async fn open_elsewhere(
        &self,
        transport: Arc<dyn Transport>,
        backend_session_id: &str,
    ) -> OpenElsewhere {
        self.adapter
            .open_elsewhere(transport, backend_session_id)
            .await
    }
}

/// The Claude adapter with a Claude home that does not exist, so nothing a
/// test does can read the user's own session records.
pub fn isolated_claude() -> ClaudeAdapter {
    let nowhere =
        std::env::temp_dir().join(format!("ersa-test-no-claude-home-{}", std::process::id()));
    ClaudeAdapter::new().with_claude_home(nowhere)
}

pub fn fixture_dir() -> PathBuf {
    fixtures_of("2.1.302")
}

/// The recorded fixtures of Claude Code `version`.
pub fn fixtures_of(version: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("docs/briefs/protocol/fixtures/claude-code")
        .join(version)
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
    let replay = Replay {
        adapter: Arc::new(isolated_claude()),
        recording: recording(),
        title: FIXTURE.to_string(),
        // What detection finds in these tests' Cores, which have no backend
        // installed; a live test Core starts its sessions with it too.
        backend_version: String::new(),
        pace,
    };
    start_replay_of(replay, configure).await
}

pub fn parse(line: &str) -> Value {
    serde_json::from_str(line).unwrap()
}

/// A stub CLI that answers each of Ersa's lines with the recording's next
/// stdout lines: `initialize`, then the prompt, then the permission answer.
pub fn stub(stdout: &[String]) -> FakeTransport {
    stub_asking_at(stdout, 4)
}

/// A stub CLI for a recording whose `can_use_tool` request is stdout line
/// `ask`: it answers `initialize`, takes the prompt, asks, and finishes the
/// turn once it has an answer.
pub fn stub_asking_at(stdout: &[String], ask: usize) -> FakeTransport {
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
        .then_stdout(&stdout[1..=ask])
        .wait_for_stdin(is("answer"))
        .then_stdout(&stdout[ask + 1..])
        .hold_open()
}
