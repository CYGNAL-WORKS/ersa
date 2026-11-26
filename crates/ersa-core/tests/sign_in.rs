//! A backend's sign-in through Core's API (PROTO-CORE-API, Signing in), over
//! real connections, against the fake backend's sign-in, which the test
//! starts, refuses, holds and ends. Each start runs a real process tree
//! through the transport Core gives the adapter, so each test sees that
//! nothing a sign-in started is left once it ended (R162).

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ersa_adapter::{
    AdapterError, AdapterSession, AdapterSink, Projector, SessionConfig, SignInEnd, TitleRules,
    Transport, async_trait,
};
use ersa_core::{Adapter, CoreConfig};
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter, ModelsAnswer, SignInMode, fake_models};
use support::procs::eventually_gone;
use support::{Client, TestCore, detecting, kind, shut_down, start_with_adapters, tempdir};

/// An address of a sign-in page, as a backend would hand one over. Made up:
/// no real sign-in address is ever written here (R163).
const PAGE: &str = "https://sign-in.example/start?state=ersa-635-marker";

/// The marker in [`PAGE`], looked for where Core must not write it.
const MARKER: &str = "ersa-635-marker";

/// A backend whose adapter has none of the trait's sign-in: it keeps the
/// default, as the Claude Code adapter does until its own is built.
struct NoSignIn(FakeAdapter);

#[async_trait]
impl Adapter for NoSignIn {
    fn backend(&self) -> &str {
        self.0.backend()
    }
    fn title_rules(&self) -> TitleRules {
        self.0.title_rules()
    }
    async fn start(
        &self,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        sink: Arc<dyn AdapterSink>,
    ) -> Result<Box<dyn AdapterSession>, AdapterError> {
        self.0.start(transport, config, sink).await
    }
    fn projector(&self) -> Box<dyn Projector> {
        self.0.projector()
    }
}

/// A Core with the fake, whose sign-in starts with `mode`, and a backend
/// without a sign-in, detecting both for real, configured by `configure`.
async fn core_with(
    mode: SignInMode,
    configure: impl FnOnce(&mut CoreConfig),
) -> (TestCore, Arc<FakeAdapter>, Client) {
    let fake = Arc::new(FakeAdapter::new(1));
    *fake.sign_ins.mode.lock().unwrap() = mode;
    let without = NoSignIn(FakeAdapter::named("without-sign-in"));
    let adapters: Vec<Arc<dyn Adapter>> =
        vec![Arc::clone(&fake) as Arc<dyn Adapter>, Arc::new(without)];
    let configure = |config: &mut CoreConfig| {
        detecting(config);
        configure(config);
    };
    let core = start_with_adapters(tempdir(), configure, adapters).await;
    let client = core.client().await;
    (core, fake, client)
}

fn the_fake() -> Value {
    json!({ "host_id": "local", "name": fake::BACKEND })
}

/// The fake's entry in `backend.list`.
async fn listed(client: &mut Client) -> Value {
    let infos = client
        .result("backend.list", json!({ "host_id": "local" }))
        .await;
    let infos = infos.as_array().expect("a list").clone();
    infos
        .into_iter()
        .find(|info| info["name"] == fake::BACKEND)
        .expect("the fake is listed")
}

/// The next `backend.sign_in_ended`, skipping other notifications.
async fn ended(client: &mut Client) -> Value {
    client
        .notification_where(|n| n["method"] == "backend.sign_in_ended")
        .await["params"]
        .clone()
}

/// Whether a `backend.sign_in_ended` has reached `client` by the time a
/// request sent now is answered.
async fn any_ended_yet(client: &mut Client) -> bool {
    client.result("core.status", json!({})).await;
    client
        .take_notifications()
        .iter()
        .any(|n| n["method"] == "backend.sign_in_ended")
}

/// `backend.sign_in` answers once the sign-in has started, with the address
/// the adapter handed over, unchanged, and without one when it handed none.
// verifies: PROTO-CORE-API-R155
#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_answers_once_started_with_the_address_its_backend_handed_over() {
    let (_core, fake, mut client) = core_with(SignInMode::Starts(Some(PAGE.into())), |_| {}).await;

    let started = client.result("backend.sign_in", the_fake()).await;
    assert_eq!(started, json!({ "url": PAGE }));
    assert_eq!(fake.sign_ins.starts.load(Ordering::SeqCst), 1);
    fake.sign_ins
        .end(SignInEnd::Failed("stopped by the test".into()));
    ended(&mut client).await;

    *fake.sign_ins.mode.lock().unwrap() = SignInMode::Starts(None);
    let started = client.result("backend.sign_in", the_fake()).await;
    assert_eq!(
        started,
        json!({}),
        "no address: opening the page is the backend's"
    );
    fake.sign_ins
        .end(SignInEnd::Failed("stopped by the test".into()));
    ended(&mut client).await;
}

/// Core opens no address itself: it has nothing that opens one.
// verifies: PROTO-CORE-API-R155
#[test]
fn core_has_nothing_that_opens_an_address() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("Core's manifest");
    // A dependency's name is what a line starts with, up to `.`, `=` or a
    // space: `tokio.workspace = true`, `time = { ... }`.
    let named: Vec<&str> = manifest
        .lines()
        .filter_map(|line| line.split(['.', '=', ' ']).next())
        .collect();
    for opener in ["open", "opener", "webbrowser", "that"] {
        assert!(
            !named.contains(&opener),
            "Core depends on {opener}, which opens addresses"
        );
    }
}

/// While one is under way, a `backend.sign_in` from any connection starts
/// no second and answers as the first did: one waiting on the start gets
/// the start's answer, one after it the same.
// verifies: PROTO-CORE-API-R156
#[tokio::test(flavor = "multi_thread")]
async fn a_second_sign_in_while_one_is_under_way_starts_none_and_answers_as_the_first() {
    let (core, fake, mut first) = core_with(SignInMode::Starts(Some(PAGE.into())), |_| {}).await;
    let mut second = core.client().await;

    let held = fake.sign_ins.start_gate.lock().await;
    let asked = first.request("backend.sign_in", the_fake()).await;
    fake.sign_ins.latest_process().await;
    let also_asked = second.request("backend.sign_in", the_fake()).await;
    drop(held);
    let answer = first.response(asked).await;
    let also = second.response(also_asked).await;
    assert_eq!(answer["result"], json!({ "url": PAGE }));
    assert_eq!(also["result"], answer["result"]);

    let again = second.result("backend.sign_in", the_fake()).await;
    assert_eq!(again, json!({ "url": PAGE }));
    assert_eq!(fake.sign_ins.starts.load(Ordering::SeqCst), 1, "one start");
    fake.sign_ins
        .end(SignInEnd::Failed("stopped by the test".into()));
    ended(&mut first).await;
}

/// A `backend.sign_in` while the one under way is ending, as a cancel stops
/// it, joins nothing on its way out: it waits for that one to end, and then
/// starts afresh, answering with the new sign-in's address.
// verifies: PROTO-CORE-API-R156
#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_asked_for_while_one_is_ending_starts_afresh_once_it_has_ended() {
    let (core, fake, mut client) = core_with(SignInMode::Starts(Some(PAGE.into())), |_| {}).await;
    let mut other = core.client().await;
    client.result("backend.sign_in", the_fake()).await;

    let stopping = fake.sign_ins.stop_gate.lock().await;
    let canceling = other.request("backend.cancel_sign_in", the_fake()).await;
    while fake.sign_ins.stops.load(Ordering::SeqCst) < 1 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let fresh = "https://sign-in.example/again";
    *fake.sign_ins.mode.lock().unwrap() = SignInMode::Starts(Some(fresh.into()));
    let asked = client.request("backend.sign_in", the_fake()).await;
    client.result("core.status", json!({})).await;
    assert_eq!(fake.sign_ins.starts.load(Ordering::SeqCst), 1, "it waits");
    drop(stopping);

    assert_eq!(other.response(canceling).await["result"], json!({}));
    assert_eq!(
        client.response(asked).await["result"],
        json!({ "url": fresh })
    );
    assert_eq!(fake.sign_ins.starts.load(Ordering::SeqCst), 2);
    assert_eq!(ended(&mut other).await["outcome"], json!("canceled"));

    // And one that ended by itself, while Core still lets go of it.
    let stopping = fake.sign_ins.stop_gate.lock().await;
    fake.sign_ins
        .end(SignInEnd::Failed("stopped by the test".into()));
    while fake.sign_ins.stops.load(Ordering::SeqCst) < 2 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    *fake.sign_ins.mode.lock().unwrap() = SignInMode::Starts(Some(PAGE.into()));
    let asked = client.request("backend.sign_in", the_fake()).await;
    client.result("core.status", json!({})).await;
    assert_eq!(fake.sign_ins.starts.load(Ordering::SeqCst), 2, "it waits");
    drop(stopping);
    assert_eq!(
        client.response(asked).await["result"],
        json!({ "url": PAGE })
    );
    assert_eq!(fake.sign_ins.starts.load(Ordering::SeqCst), 3);
    assert_eq!(ended(&mut other).await["outcome"], json!("failed"));
    fake.sign_ins
        .end(SignInEnd::Failed("stopped by the test".into()));
    ended(&mut other).await;
}

/// Core's own limits: 20 seconds for a sign-in to start, 10 minutes for
/// it to end, unless a test sets its own.
// verifies: PROTO-CORE-API-R157
// verifies: PROTO-CORE-API-R161
#[test]
fn a_sign_in_has_20_seconds_to_start_and_10_minutes_to_end() {
    let dir = tempdir();
    let config = CoreConfig::new(dir.path().to_path_buf()).expect("a configuration");
    assert_eq!(config.sign_in_start_limit, Duration::from_secs(20));
    assert_eq!(config.sign_in_limit, Duration::from_secs(10 * 60));
}

/// The 20 seconds count from the request: one held behind a sign-in that
/// is ending, which takes longer to end than the limit, is answered
/// `invalid_state` when its time is up, still waiting, and starts nothing.
// verifies: PROTO-CORE-API-R157
#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_held_behind_one_that_is_ending_answers_within_its_limit() {
    let limit = |config: &mut CoreConfig| config.sign_in_start_limit = Duration::from_secs(2);
    let (core, fake, mut client) = core_with(SignInMode::Starts(Some(PAGE.into())), limit).await;
    let mut other = core.client().await;
    client.result("backend.sign_in", the_fake()).await;

    let stopping = fake.sign_ins.stop_gate.lock().await;
    let canceling = other.request("backend.cancel_sign_in", the_fake()).await;
    while fake.sign_ins.stops.load(Ordering::SeqCst) < 1 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Answered while the one before it still has not ended.
    let error = client.error("backend.sign_in", the_fake()).await;
    assert_eq!(kind(&error), "invalid_state");
    assert_eq!(
        error["message"],
        json!("The sign-in to fake didn't start within 2 seconds.")
    );
    drop(stopping);
    assert_eq!(other.response(canceling).await["result"], json!({}));
    assert_eq!(ended(&mut other).await["outcome"], json!("canceled"));
    assert_eq!(
        fake.sign_ins.starts.load(Ordering::SeqCst),
        1,
        "it started nothing"
    );
    assert_eq!(listed(&mut other).await.get("signing_in"), None);
}

/// `backend_missing` for a backend Core has no adapter for, and
/// `invalid_state` when its command is not found, when its adapter has no
/// sign-in, when the backend refuses, with its own reason, and when the
/// sign-in has not started within the limit; each leaving nothing it
/// started running, and none of them a sign-in under way.
// verifies: PROTO-CORE-API-R157
#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_that_cannot_start_is_refused_and_leaves_nothing_running() {
    // Long enough for the fake's process to start on a busy machine, short
    // enough to wait out: the test waits on the answer, not on a pause.
    let limit = |config: &mut CoreConfig| config.sign_in_start_limit = Duration::from_secs(5);
    let (_core, fake, mut client) = core_with(SignInMode::NotFound, limit).await;

    let error = client
        .error(
            "backend.sign_in",
            json!({ "host_id": "local", "name": "nobody" }),
        )
        .await;
    assert_eq!(kind(&error), "backend_missing");
    let error = client
        .error(
            "backend.sign_in",
            json!({ "host_id": "elsewhere", "name": fake::BACKEND }),
        )
        .await;
    assert_eq!(kind(&error), "not_found");

    let error = client.error("backend.sign_in", the_fake()).await;
    assert_eq!(kind(&error), "invalid_state");
    assert!(
        error["message"].as_str().unwrap().contains("wasn't found"),
        "{error}"
    );

    let without = json!({ "host_id": "local", "name": "without-sign-in" });
    let error = client.error("backend.sign_in", without).await;
    assert_eq!(kind(&error), "invalid_state");
    assert_eq!(
        error["message"],
        json!("Ersa can't start a sign-in to without-sign-in")
    );

    let reason = "The vendor's sign-in is closed for maintenance.";
    *fake.sign_ins.mode.lock().unwrap() = SignInMode::Refuses(reason.into());
    let error = client.error("backend.sign_in", the_fake()).await;
    assert_eq!(kind(&error), "invalid_state");
    assert_eq!(error["message"], json!(reason), "the backend's own reason");
    eventually_gone(fake.sign_ins.latest_process().await).await;

    *fake.sign_ins.mode.lock().unwrap() = SignInMode::NeverStarts;
    let error = client.error("backend.sign_in", the_fake()).await;
    assert_eq!(kind(&error), "invalid_state");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("didn't start within 5 seconds"),
        "{error}"
    );
    eventually_gone(fake.sign_ins.latest_process().await).await;

    // None of them was under way for anyone, and none is said to have ended.
    assert_eq!(listed(&mut client).await.get("signing_in"), None);
    assert!(!any_ended_yet(&mut client).await);
}

/// `backend.list` carries `signing_in` while a sign-in Ersa started is
/// under way, with the address it answered, or none, and nothing before it
/// started or after it ended.
// verifies: PROTO-CORE-API-R158
#[tokio::test(flavor = "multi_thread")]
async fn the_backends_entry_carries_the_sign_in_while_it_is_under_way() {
    let (core, fake, mut client) = core_with(SignInMode::Starts(Some(PAGE.into())), |_| {}).await;
    let mut asker = core.client().await;
    assert_eq!(listed(&mut client).await.get("signing_in"), None);

    let held = fake.sign_ins.start_gate.lock().await;
    let asked = asker.request("backend.sign_in", the_fake()).await;
    fake.sign_ins.latest_process().await;
    // Not yet started: nobody else is told of it until it has.
    assert_eq!(listed(&mut client).await.get("signing_in"), None);
    drop(held);
    asker.response(asked).await;
    assert_eq!(
        listed(&mut client).await["signing_in"],
        json!({ "url": PAGE })
    );

    fake.sign_ins
        .end(SignInEnd::Failed("stopped by the test".into()));
    ended(&mut client).await;
    assert_eq!(listed(&mut client).await.get("signing_in"), None);

    *fake.sign_ins.mode.lock().unwrap() = SignInMode::Starts(None);
    asker.result("backend.sign_in", the_fake()).await;
    assert_eq!(listed(&mut client).await["signing_in"], json!({}));
    fake.sign_ins
        .end(SignInEnd::Failed("stopped by the test".into()));
    ended(&mut client).await;
}

/// Each ending is sent once to every connection past `core.hello`, the one
/// that asked and one that did not alike, with how it ended and the
/// backend's own reason for a failure; two endings are two notifications,
/// never one.
// verifies: PROTO-CORE-API-R159
#[tokio::test(flavor = "multi_thread")]
async fn each_ending_is_sent_once_to_every_connection() {
    let (core, fake, mut asker) = core_with(SignInMode::Starts(Some(PAGE.into())), |_| {}).await;
    let mut other = core.client().await;

    asker.result("backend.sign_in", the_fake()).await;
    fake.sign_ins
        .end(SignInEnd::Failed("The code expired.".into()));
    let expected = json!({
        "host_id": "local", "name": fake::BACKEND, "outcome": "failed", "message": "The code expired."
    });
    assert_eq!(ended(&mut asker).await, expected);
    eventually_gone(fake.sign_ins.latest_process().await).await;

    // A second sign-in ends while `other` reads nothing: it gets both, in
    // order, each once.
    asker.result("backend.sign_in", the_fake()).await;
    fake.sign_ins.end(SignInEnd::SignedIn);
    let signed_in = ended(&mut asker).await;
    assert_eq!(signed_in["outcome"], json!("signed_in"));
    assert_eq!(signed_in.get("message"), None);
    assert_eq!(ended(&mut other).await, expected);
    assert_eq!(ended(&mut other).await, signed_in);
    assert!(!any_ended_yet(&mut asker).await, "once to the asker");
    assert!(!any_ended_yet(&mut other).await, "once to the other");
    eventually_gone(fake.sign_ins.latest_process().await).await;
}

/// Before saying a sign-in ended `signed_in`, Core detects the backend
/// again, whatever it kept: an `available` backend that now says it is
/// signed out reads so straight after, and one signed out that now says it
/// is signed in was asked before the notification.
// verifies: PROTO-CORE-API-R159
#[tokio::test(flavor = "multi_thread")]
async fn core_detects_the_backend_again_before_saying_it_signed_in() {
    let (_core, fake, mut client) = core_with(SignInMode::Starts(None), |_| {}).await;
    assert_eq!(listed(&mut client).await["status"], json!("available"));

    *fake.models.lock().unwrap() = ModelsAnswer::SignedOut(fake_models());
    client.result("backend.sign_in", the_fake()).await;
    fake.sign_ins.end(SignInEnd::SignedIn);
    ended(&mut client).await;
    assert_eq!(
        listed(&mut client).await["status"],
        json!("signed_out"),
        "the available backend Core kept was detected again"
    );

    client.result("backend.sign_in", the_fake()).await;
    *fake.models.lock().unwrap() = ModelsAnswer::Listed(fake_models());
    let before = fake.stats.model_probes.load(Ordering::SeqCst);
    fake.sign_ins.end(SignInEnd::SignedIn);
    ended(&mut client).await;
    assert_eq!(fake.stats.model_probes.load(Ordering::SeqCst), before + 1);
    assert_eq!(listed(&mut client).await["status"], json!("available"));
}

/// `backend.cancel_sign_in` stops the sign-in under way, which ends
/// `canceled` with nothing it started left; with none under way it
/// succeeds and sends nothing; and a sign-in canceled before it started
/// never starts for anyone.
// verifies: PROTO-CORE-API-R160
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_stops_the_sign_in_under_way_and_does_nothing_otherwise() {
    let (core, fake, mut client) = core_with(SignInMode::Starts(Some(PAGE.into())), |_| {}).await;
    let mut other = core.client().await;

    assert_eq!(
        client.result("backend.cancel_sign_in", the_fake()).await,
        json!({})
    );
    let nobody = json!({ "host_id": "local", "name": "nobody" });
    assert_eq!(
        client.result("backend.cancel_sign_in", nobody).await,
        json!({})
    );
    assert!(!any_ended_yet(&mut client).await, "nothing was under way");

    client.result("backend.sign_in", the_fake()).await;
    let process = fake.sign_ins.latest_process().await;
    assert_eq!(
        other.result("backend.cancel_sign_in", the_fake()).await,
        json!({})
    );
    eventually_gone(process).await;
    let canceled = ended(&mut client).await;
    assert_eq!(
        canceled,
        json!({ "host_id": "local", "name": fake::BACKEND, "outcome": "canceled" })
    );
    assert_eq!(ended(&mut other).await, canceled);
    assert_eq!(fake.sign_ins.stops.load(Ordering::SeqCst), 1);

    // Canceled while it starts: its request is told so, nobody else hears
    // of it, and what it ran is gone.
    let held = fake.sign_ins.start_gate.lock().await;
    let asked = client.request("backend.sign_in", the_fake()).await;
    let process = loop {
        let process = fake.sign_ins.latest_process().await;
        if fake.sign_ins.processes.lock().unwrap().len() == 2 {
            break process;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    // The cancel answers once what the start ran is gone, the start still
    // held where it was.
    assert_eq!(
        other.result("backend.cancel_sign_in", the_fake()).await,
        json!({})
    );
    drop(held);
    let answer = client.response(asked).await;
    assert_eq!(kind(&answer["error"]), "invalid_state");
    assert!(
        answer["error"]["message"]
            .as_str()
            .unwrap()
            .contains("canceled")
    );
    eventually_gone(process).await;
    assert!(!any_ended_yet(&mut other).await);
    assert_eq!(listed(&mut other).await.get("signing_in"), None);
}

/// A sign-in still under way when its limit has run, counted from its
/// start, ends `timed_out`, with nothing it started left. The test sets the
/// limit, and waits on the notification.
// verifies: PROTO-CORE-API-R161
#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_that_runs_past_its_limit_ends_timed_out() {
    let limit = |config: &mut CoreConfig| config.sign_in_limit = Duration::from_millis(300);
    let (_core, fake, mut client) = core_with(SignInMode::Starts(None), limit).await;

    client.result("backend.sign_in", the_fake()).await;
    let process = fake.sign_ins.latest_process().await;
    let timed_out = ended(&mut client).await;
    assert_eq!(timed_out["outcome"], json!("timed_out"));
    eventually_gone(process).await;
    assert_eq!(fake.sign_ins.stops.load(Ordering::SeqCst), 1);
}

/// `core.shutdown` ends a sign-in under way, and nothing it started
/// outlives Core.
// verifies: PROTO-CORE-API-R162
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_ends_a_sign_in_under_way_and_leaves_nothing_running() {
    let (core, fake, mut client) = core_with(SignInMode::Starts(Some(PAGE.into())), |_| {}).await;
    client.result("backend.sign_in", the_fake()).await;
    let process = fake.sign_ins.latest_process().await;
    drop(client);

    shut_down(core).await;
    eventually_gone(process).await;
    assert_eq!(fake.sign_ins.stops.load(Ordering::SeqCst), 1);
}

/// However a sign-in ends, nothing it started is left: after a failure, a
/// sign-in, and one its backend never stopped by itself.
// verifies: PROTO-CORE-API-R162
#[tokio::test(flavor = "multi_thread")]
async fn nothing_a_sign_in_started_is_left_after_it_ends() {
    let (_core, fake, mut client) = core_with(SignInMode::Starts(None), |_| {}).await;
    for end in [SignInEnd::Failed("No.".into()), SignInEnd::SignedIn] {
        client.result("backend.sign_in", the_fake()).await;
        let process = fake.sign_ins.latest_process().await;
        fake.sign_ins.end(end);
        ended(&mut client).await;
        eventually_gone(process).await;
    }
}

/// Core writes nothing of what a sign-in exchanges, its page's address
/// included, to the diagnostic log or the app log, and stores nothing of
/// it: after sign-ins that end each way, nothing Core wrote holds it.
// verifies: PROTO-CORE-API-R163
#[tokio::test(flavor = "multi_thread")]
async fn core_writes_and_stores_nothing_of_a_sign_in() {
    let log = support::log::Log::installed();
    let limit = |config: &mut CoreConfig| config.sign_in_limit = Duration::from_millis(300);
    let (core, fake, mut client) = core_with(SignInMode::Starts(Some(PAGE.into())), limit).await;

    let failed = SignInEnd::Failed(format!("The page {PAGE} expired."));
    for end in [Some(failed), Some(SignInEnd::SignedIn), None] {
        assert_eq!(
            client.result("backend.sign_in", the_fake()).await,
            json!({ "url": PAGE })
        );
        listed(&mut client).await;
        // `None` is left to run out its limit.
        if let Some(end) = end {
            fake.sign_ins.end(end);
        }
        ended(&mut client).await;
    }
    client.result("backend.sign_in", the_fake()).await;
    client.result("backend.cancel_sign_in", the_fake()).await;
    ended(&mut client).await;
    drop(client);
    let data = core.data_dir();
    // Kept until the files are read: dropping it deletes the folder.
    let _dir = shut_down(core).await;

    assert!(
        !log.contains(MARKER),
        "the diagnostic log holds the address"
    );
    let mut folders = vec![data];
    let mut files = 0;
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            files += 1;
            let bytes = std::fs::read(&path).unwrap();
            let holds = bytes.windows(MARKER.len()).any(|w| w == MARKER.as_bytes());
            assert!(!holds, "{} holds the address", path.display());
        }
    }
    assert!(files > 0, "the app log and the database were read");
}

/// Neither method holds up another request on its connection: while a
/// sign-in's start is held, the same connection is answered other
/// requests, a cancel included.
// verifies: PROTO-CORE-API-R164
#[tokio::test(flavor = "multi_thread")]
async fn neither_method_holds_up_other_requests_on_its_connection() {
    let (_core, fake, mut client) = core_with(SignInMode::Starts(None), |_| {}).await;
    let held = fake.sign_ins.start_gate.lock().await;
    let asked = client.request("backend.sign_in", the_fake()).await;
    fake.sign_ins.latest_process().await;

    client.result("core.status", json!({})).await;
    client
        .result("backend.list", json!({ "host_id": "local" }))
        .await;
    let other = json!({ "host_id": "local", "name": "without-sign-in" });
    assert_eq!(
        client.result("backend.cancel_sign_in", other).await,
        json!({})
    );
    assert_eq!(
        client.responses_waiting(),
        0,
        "the sign-in is still starting"
    );

    // A cancel that waits for the sign-in to stop holds nothing up either.
    let canceling = client.request("backend.cancel_sign_in", the_fake()).await;
    client.result("core.status", json!({})).await;
    drop(held);
    client.response(canceling).await;
    client.response(asked).await;
}
