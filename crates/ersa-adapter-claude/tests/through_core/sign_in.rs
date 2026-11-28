//! Claude Code's sign-in through Core: the real adapter on a stand-in CLI
//! playing the recorded lines, over Core's real socket, so the adapter and
//! Core agree about when a sign-in has started and when it has ended.

use std::sync::Arc;

use ersa_adapter::fake::FakeTransport;
use ersa_adapter::recording::Recording;
use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, BackendChoices, ModelReport, ProcessSpec,
    Projector, SessionConfig, SignInStartError, StartedSignIn, TitleRules, Transport, async_trait,
};
use ersa_adapter_claude::{ClaudeAdapter, read_models};
use ersa_core::{CommandTransport, CoreConfig};
use serde_json::json;

use crate::common::stand_in::{Script, StandIn, Step, parse};
use crate::common::{fixtures_root, sign_in_recording};
use crate::support::{detecting, shut_down, start_with_adapters, tempdir};

/// The real Claude adapter, its model probe and its sign-in run on the
/// stand-in instead of the transport Core hands it. It starts no session.
struct OnStandIn {
    adapter: ClaudeAdapter,
    stand_in: StandIn,
}

#[async_trait]
impl Adapter for OnStandIn {
    fn backend(&self) -> &str {
        self.adapter.backend()
    }

    fn label(&self) -> &str {
        self.adapter.label()
    }

    fn tested_versions(&self) -> Option<(&'static str, &'static str)> {
        self.adapter.tested_versions()
    }

    fn choices(&self) -> BackendChoices {
        self.adapter.choices()
    }

    fn set_environment(&self, spec: &mut ProcessSpec) {
        self.adapter.set_environment(spec)
    }

    /// The test binary itself, a file that is surely there, so Core's look
    /// for the command finds it; the version check then runs on the
    /// command transport the test gives Core, which prints the version.
    fn version_command(&self) -> ProcessSpec {
        ProcessSpec {
            program: std::env::current_exe()
                .expect("the test binary's path")
                .to_string_lossy()
                .into_owned(),
            args: vec!["--version".into()],
            ..ProcessSpec::default()
        }
    }

    fn title_rules(&self) -> TitleRules {
        self.adapter.title_rules()
    }

    fn projector(&self) -> Box<dyn Projector> {
        self.adapter.projector()
    }

    async fn start(
        &self,
        _: Arc<dyn Transport>,
        _: SessionConfig,
        _: Arc<dyn AdapterSink>,
    ) -> Result<Box<dyn AdapterSession>, AdapterError> {
        Err(AdapterError::Protocol("this test starts no session".into()))
    }

    async fn models(&self, _: Arc<dyn Transport>) -> Result<ModelReport, AdapterError> {
        self.adapter.models(Arc::new(self.stand_in.clone())).await
    }

    async fn start_sign_in(
        &self,
        _: Arc<dyn Transport>,
    ) -> Result<StartedSignIn, SignInStartError> {
        self.adapter
            .start_sign_in(Arc::new(self.stand_in.clone()))
            .await
    }
}

fn probe(name: &str) -> Recording {
    Recording::load(
        &fixtures_root()
            .join("2.1.330")
            .join(format!("{name}.jsonl")),
    )
    .expect("the probe recording loads")
}

/// The model ids a probe recording's `initialize` answer lists.
fn models_of(recording: &Recording) -> Vec<String> {
    let answer = recording
        .stdout()
        .iter()
        .map(|line| parse(line))
        .find(|line| line["response"]["response"]["models"].is_array())
        .expect("the probe lists models");
    read_models(&answer["response"]["response"])
        .expect("models")
        .into_iter()
        .map(|model| model.id)
        .collect()
}

/// `backend.sign_in` answers with the recorded address once the adapter has
/// started the sign-in; once the stand-in answers the wait,
/// `backend.sign_in_ended` says `signed_in`, and the listing read after it
/// reports the backend as the next probe answers, signed in as fixture
/// 08-sign-in-finished-probe is. Nothing is left running, and Core's
/// folder holds nothing of the address.
// verifies: PROTO-CLAUDE-CODE-STREAM-R85
// verifies: PROTO-CLAUDE-CODE-STREAM-R87
#[tokio::test(flavor = "multi_thread")]
async fn a_recorded_sign_in_through_core_starts_and_ends_signed_in() {
    let signed_out = probe("00-probe-get-settings");
    let signed_in_recording = sign_in_recording("2.1.330", "08-sign-in-finished-probe");
    let finished = sign_in_recording("2.1.330", "06-sign-in-finished");
    // The wait's answer is held until the test has read Core's answer.
    let sign_in =
        Script::recorded(&finished).before_answer("claude_oauth_wait_for_completion", [Step::Gate]);
    let stand_in = StandIn::new([
        Script::recorded(&signed_out),
        sign_in,
        Script::recorded(&signed_in_recording),
    ]);
    let claude: Arc<dyn Adapter> = Arc::new(OnStandIn {
        adapter: ClaudeAdapter::new(),
        stand_in: stand_in.clone(),
    });
    let configure = |config: &mut CoreConfig| {
        detecting(config);
        config.command_transport = Some(CommandTransport(Arc::new(FakeTransport::new([
            "2.1.330 (Claude Code)",
        ]))));
    };
    let core = start_with_adapters(tempdir(), configure, vec![claude]).await;
    let mut client = core.client().await;
    let local = json!({ "host_id": "local" });
    let claude_code = json!({ "host_id": "local", "name": "claude-code" });

    let listed = client.result("backend.list", local.clone()).await;
    assert_eq!(listed[0]["status"], "signed_out", "{listed:#}");

    let address = Script::recorded(&finished).answer("claude_authenticate", |_| {});
    let address = parse(&address)["response"]["response"]["automaticUrl"].clone();
    let started = client.result("backend.sign_in", claude_code.clone()).await;
    assert_eq!(started, json!({ "url": address }));

    stand_in.release(1);
    let ended = client
        .notification_where(|n| n["method"] == "backend.sign_in_ended")
        .await;
    assert_eq!(
        ended["params"],
        json!({ "host_id": "local", "name": "claude-code", "outcome": "signed_in" })
    );

    let listed = client.result("backend.list", local).await;
    assert_eq!(listed[0]["status"], "available", "{listed:#}");
    assert!(listed[0].get("signing_in").is_none(), "{listed:#}");
    let ids: Vec<String> = listed[0]["models"]
        .as_array()
        .expect("models")
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, models_of(&signed_in_recording));

    assert_eq!(stand_in.spawned().len(), 3);
    assert_eq!(stand_in.running(), 0);
    assert!(stand_in.failures().is_empty(), "{:#?}", stand_in.failures());

    drop(client);
    let data = core.data_dir();
    let _dir = shut_down(core).await;
    let address = address.as_str().unwrap().as_bytes().to_vec();
    let mut folders = vec![data];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            let holds = bytes.windows(address.len()).any(|w| w == address);
            assert!(!holds, "{} holds the address", path.display());
        }
    }
}
