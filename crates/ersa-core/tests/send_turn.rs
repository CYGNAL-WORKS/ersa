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
async fn running_session(core: &TestCore, client: &mut Client) -> String {
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
/// 5 seconds until the answer.
fn assert_reported_throughout(sent: Instant, answered: Instant, reports: &[(String, Instant)]) {
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
        answered.duration_since(sent) >= HELD,
        "the send was not held: answered after {:?}",
        answered.duration_since(sent)
    );
}

/// A send behind a stop on the same connection waits for the stop, which
/// takes a while, and reports `checking` meanwhile.
// verifies: PROTO-CORE-API-R90
#[tokio::test(flavor = "multi_thread")]
async fn a_send_behind_an_earlier_request_reports_checking() {
    let core = start_fake(FakeAdapter::slow_stop(HELD), |_| {}).await;
    let mut client = core.client().await;
    let id = running_session(&core, &mut client).await;

    client.request("session.stop", json!({ "id": id })).await;
    let sent = Instant::now();
    let send = client
        .request("session.send", json!({ "id": id, "text": "again" }))
        .await;
    let (answer, answered, reports) = reports_until_answer(&mut client, send, &id).await;
    assert!(answer.get("result").is_some(), "{answer:#}");
    assert_reported_throughout(sent, answered, &reports);
}

/// A send to a session whose run is winding down waits for it to end, and
/// reports `checking` meanwhile.
// verifies: PROTO-CORE-API-R90
#[tokio::test(flavor = "multi_thread")]
async fn a_send_waiting_for_a_run_winding_down_reports_checking() {
    let core = start_fake(FakeAdapter::new(1), |config| config.wind_down_pause = HELD).await;
    let mut client = core.client().await;
    let id = running_session(&core, &mut client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "exit" }))
        .await;
    client.events_until("session.ended").await;

    let sent = Instant::now();
    let send = client
        .request("session.send", json!({ "id": id, "text": "again" }))
        .await;
    let (answer, answered, reports) = reports_until_answer(&mut client, send, &id).await;
    assert!(answer.get("result").is_some(), "{answer:#}");
    assert_reported_throughout(sent, answered, &reports);
}
