//! A send still waiting its turn reports where it is (PROTO-CORE-API-R90):
//! from when Core receives it, `session.send_progress` says `checking` at
//! least every 5 seconds, whether it waits behind an earlier request on its
//! session or for a run that is winding down. Through the Core API with the
//! fake adapter (`support/fake.rs`), on the real clock.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ersa_core::Adapter;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter};
use support::{Client, TestCore, start_with_adapters, tempdir, workspace};
use tokio::time::Instant;

/// How long each wait in these tests holds the send: longer than the 5
/// seconds between reports, so a send that reports nothing while it waits
/// is caught.
const HELD: Duration = Duration::from_secs(9);

/// The slack a busy machine gets over 5 seconds between reports, and over
/// "at once" for the first one.
const SLACK: Duration = Duration::from_millis(800);
const EVERY: Duration = Duration::from_secs(5);

async fn start_fake(
    adapter: FakeAdapter,
    configure: impl FnOnce(&mut ersa_core::CoreConfig),
) -> TestCore {
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(adapter)];
    start_with_adapters(tempdir(), configure, adapters).await
}

/// A running session of the fake backend that has had one turn; its id.
///
/// Set up on a connection of its own, which the test then leaves. The
/// `session.send_progress` of that first turn's send are per session, not
/// per send, and reach a connection late (only the latest per session is
/// kept for it), so on the test's connection they could pass for the
/// reports the test waits on. A connection opened after the send's answer
/// gets none of them: its reports begin when it says hello.
async fn running_session(core: &TestCore) -> String {
    let mut setup = core.client().await;
    let client = &mut setup;
    let workspace_id = workspace(client, core.dir.path()).await;
    let id = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": fake::BACKEND }),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    id
}

/// Reads until the answer to request `request`, timing each
/// `session.send_progress` for `session` on the way; returns the answer,
/// when it came, and each report's phase and time.
async fn reports_until_answer(
    client: &mut Client,
    request: i64,
    session: &str,
) -> (Value, Instant, Vec<(String, Instant)>) {
    let mut reports = Vec::new();
    loop {
        let line = client.read_line().await.expect("the connection stays open");
        let now = Instant::now();
        if line["id"] == request {
            return (line, now, reports);
        }
        if line["method"] == "session.send_progress" && line["params"]["session_id"] == session {
            let phase = line["params"]["phase"].as_str().unwrap().to_string();
            reports.push((phase, now));
        }
    }
}

/// The reports start at once, say `checking` first, and come at least every
/// 5 seconds until the answer, which came no sooner than `HELD` after
/// `holding`: a moment before the test asked for what holds the send, so
/// the wait inside Core cannot have begun before it.
fn assert_reported_throughout(
    holding: Instant,
    sent: Instant,
    answered: Instant,
    reports: &[(String, Instant)],
) {
    let (first_phase, first) = reports.first().expect("a report while the send waited");
    assert_eq!(first_phase, "checking", "{reports:?}");
    assert!(
        first.duration_since(sent) <= SLACK,
        "the first report came {:?} after the send",
        first.duration_since(sent)
    );
    let times: Vec<Instant> = reports
        .iter()
        .map(|(_, at)| *at)
        .chain([answered])
        .collect();
    for pair in times.windows(2) {
        let gap = pair[1].duration_since(pair[0]);
        assert!(
            gap <= EVERY + SLACK,
            "{gap:?} without a report: {reports:?}"
        );
    }
    assert!(
        answered.duration_since(holding) >= HELD,
        "the send was not held: answered {:?} after what holds it was asked for",
        answered.duration_since(holding)
    );
}

/// A send behind a stop on the same connection waits for the stop, which
/// takes a while, and reports `checking` meanwhile.
// verifies: PROTO-CORE-API-R90
#[tokio::test(flavor = "multi_thread")]
async fn a_send_behind_an_earlier_request_reports_checking() {
    let core = start_fake(FakeAdapter::slow_stop(HELD), |_| {}).await;
    let id = running_session(&core).await;
    let mut client = core.client().await;

    let holding = Instant::now();
    client.request("session.stop", json!({ "id": id })).await;
    let sent = Instant::now();
    let send = client
        .request("session.send", json!({ "id": id, "text": "again" }))
        .await;
    let (answer, answered, reports) = reports_until_answer(&mut client, send, &id).await;
    assert!(answer.get("result").is_some(), "{answer:#}");
    assert_reported_throughout(holding, sent, answered, &reports);
}

/// A send waiting its turn while another connection's send has the
/// session's slot: that send reports for the session, and when it ends the
/// waiting one takes the reports over at once (PROTO-CORE-API,
/// `session.send_progress`), well inside its own pace of five seconds.
// verifies: PROTO-CORE-API-R90
#[tokio::test(flavor = "multi_thread")]
async fn a_waiting_send_reports_at_once_when_the_send_ahead_of_it_ends() {
    let core = start_fake(FakeAdapter::slow_stop(HELD), |_| {}).await;
    let id = running_session(&core).await;
    // Every report this connection reads is one of the sends below: the
    // interrupt must find the send ahead handed over, or it cancels that
    // send instead (PROTO-CORE-API-R86).
    let mut client = core.client().await;
    let reports = |phase: &'static str| {
        let id = id.clone();
        move |line: &Value| {
            line["method"] == "session.send_progress"
                && line["params"]["session_id"] == id.as_str()
                && line["params"]["phase"] == phase
        }
    };

    // Held behind a stop on its own connection, the send reports `checking`.
    client.request("session.stop", json!({ "id": id })).await;
    client
        .request("session.send", json!({ "id": id, "text": "again" }))
        .await;
    client.notification_where(reports("checking")).await;

    // Another connection's send takes the session's slot, and reports for
    // the session: its backend is slow to take the message.
    let mut other = core.client().await;
    let ahead = other
        .request(
            "session.send",
            json!({ "id": id, "text": "taken once interrupted" }),
        )
        .await;
    client.notification_where(reports("handing_over")).await;

    // It ends. The waiting send reports at once, `checking` again, where
    // its own pace would have it wait seconds more.
    let mut third = core.client().await;
    third.result("session.interrupt", json!({ "id": id })).await;
    let answered = other.response(ahead).await;
    assert!(answered.get("result").is_some(), "{answered:#}");
    let ended = Instant::now();
    client.notification_where(reports("checking")).await;
    assert!(
        ended.elapsed() <= SLACK,
        "the waiting send reported {:?} after the send ahead of it ended",
        ended.elapsed()
    );
}

/// A send to a session whose run is winding down waits for it to end, and
/// reports `checking` meanwhile.
// verifies: PROTO-CORE-API-R90
#[tokio::test(flavor = "multi_thread")]
async fn a_send_waiting_for_a_run_winding_down_reports_checking() {
    let core = start_fake(FakeAdapter::new(1), |config| config.wind_down_pause = HELD).await;
    let id = running_session(&core).await;
    // The run ends on a connection the test then leaves, as in
    // `running_session`: the `exit` send's reports are not this send's.
    let mut setup = core.client().await;
    setup
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    // The wind-down begins once the backend has exited, after this.
    let holding = Instant::now();
    setup
        .result("session.send", json!({ "id": id, "text": "exit" }))
        .await;
    setup.events_until("session.ended").await;
    let mut client = core.client().await;

    let sent = Instant::now();
    let send = client
        .request("session.send", json!({ "id": id, "text": "again" }))
        .await;
    let (answer, answered, reports) = reports_until_answer(&mut client, send, &id).await;
    assert!(answer.get("result").is_some(), "{answer:#}");
    assert_reported_throughout(holding, sent, answered, &reports);
}
