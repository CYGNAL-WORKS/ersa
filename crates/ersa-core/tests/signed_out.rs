//! A backend that is installed and answering, and says it is signed out
//! (PROTO-CORE-API, `backend.list` and Signing in). The fake backend's probe
//! answer says so, as Claude Code's `initialize` answer does.

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use ersa_core::Adapter;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter, ModelsAnswer, Stats, fake_models};
use support::{Client, TestCore, start_with_adapters, tempdir, wait_for_state, workspace};

/// A Core that detects the fake backend for real, the fake signed out.
async fn signed_out_core() -> (TestCore, Arc<FakeAdapter>, Client) {
    let fake = Arc::new(FakeAdapter::new(1));
    *fake.models.lock().unwrap() = ModelsAnswer::SignedOut(fake_models());
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::clone(&fake) as Arc<dyn Adapter>];
    let core = start_with_adapters(tempdir(), support::detecting, adapters).await;
    let client = core.client().await;
    (core, fake, client)
}

async fn backend_list_entry(client: &mut Client) -> Value {
    let infos = client
        .result("backend.list", json!({ "host_id": "local" }))
        .await;
    infos[0].clone()
}

async fn host_list_entry(client: &mut Client) -> Value {
    let hosts = client.result("host.list", json!({})).await;
    hosts[0]["backends"][0].clone()
}

fn probes(stats: &Stats) -> usize {
    stats.model_probes.load(Ordering::SeqCst)
}

/// `backend.list` and `host.list` read `signed_out`, with the version read
/// and the models the backend reported, every field as it gave them, and
/// the default effort it named marked, as for an available backend.
// verifies: PROTO-CORE-API-R152
#[tokio::test(flavor = "multi_thread")]
async fn a_backend_that_says_it_is_signed_out_lists_as_signed_out_with_its_models() {
    let (_core, _fake, mut client) = signed_out_core().await;

    let info = backend_list_entry(&mut client).await;
    assert_eq!(info["status"], json!("signed_out"));
    assert_eq!(info["version"], json!("1.0.0"));
    assert_eq!(
        info["models"],
        json!([
            { "id": "model-a", "label": "MODEL-A", "default": true },
            { "id": "model-b", "label": "MODEL-B", "default": false, "resolved": "model-b-1", "efforts": ["low"] },
        ])
    );
    let marked: Vec<&Value> = info["efforts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["default"] == json!(true))
        .map(|e| &e["id"])
        .collect();
    assert_eq!(marked, [&json!("high")]);

    let status = host_list_entry(&mut client).await;
    assert_eq!(
        status,
        json!({ "name": fake::BACKEND, "version": "1.0.0", "status": "signed_out" })
    );
}

/// Core keeps no copy of signed out: every `backend.list`, `host.list` and
/// `session.create` asks the backend again, one right after another, well
/// within the time a failed detection is kept. Once the backend says it is
/// signed in, the next reading lists it available, and that is kept.
// verifies: PROTO-CORE-API-R153
#[tokio::test(flavor = "multi_thread")]
async fn a_signed_out_backend_is_asked_again_at_every_reading() {
    let (core, fake, mut client) = signed_out_core().await;

    assert_eq!(
        backend_list_entry(&mut client).await["status"],
        json!("signed_out")
    );
    assert_eq!(probes(&fake.stats), 1);
    assert_eq!(
        backend_list_entry(&mut client).await["status"],
        json!("signed_out")
    );
    assert_eq!(probes(&fake.stats), 2);
    assert_eq!(
        host_list_entry(&mut client).await["status"],
        json!("signed_out")
    );
    assert_eq!(probes(&fake.stats), 3);
    let workspace_id = workspace(&mut client, core.dir.path()).await;
    let before = probes(&fake.stats);
    client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": fake::BACKEND }),
        )
        .await;
    assert_eq!(probes(&fake.stats), before + 1, "session.create asks again");

    // Signed in elsewhere: the very next reading shows it.
    *fake.models.lock().unwrap() = ModelsAnswer::Listed(fake_models());
    assert_eq!(
        backend_list_entry(&mut client).await["status"],
        json!("available")
    );
    let after = probes(&fake.stats);
    assert_eq!(
        host_list_entry(&mut client).await["status"],
        json!("available")
    );
    assert_eq!(probes(&fake.stats), after, "an available backend is kept");
}

/// Nothing is refused because the backend was last found signed out: a
/// session of it is created, sent to, forked, and resumed, and the backend
/// is left to say what fails.
// verifies: PROTO-CORE-API-R154
#[tokio::test(flavor = "multi_thread")]
async fn nothing_is_refused_because_the_backend_was_found_signed_out() {
    let (core, fake, mut client) = signed_out_core().await;
    assert_eq!(
        backend_list_entry(&mut client).await["status"],
        json!("signed_out")
    );

    let workspace_id = workspace(&mut client, core.dir.path()).await;
    let session = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": fake::BACKEND, "model": "model-b" }),
        )
        .await;
    let id = session["id"].as_str().unwrap().to_string();
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.started").await;
    client
        .result("session.send", json!({ "id": id, "text": "idle" }))
        .await;
    wait_for_state(&mut client, &id, "idle").await;

    let fork = client.result("session.fork", json!({ "id": id })).await;
    assert!(fork["id"].is_string(), "{fork}");

    client.result("session.stop", json!({ "id": id })).await;
    let resumed = client.result("session.resume", json!({ "id": id })).await;
    assert_eq!(resumed["id"], json!(id));
    assert_eq!(
        backend_list_entry(&mut client).await["status"],
        json!("signed_out"),
        "still signed out throughout"
    );
    // A start for each: the session, its fork, and the resume. The resume
    // answers while its backend is still starting, so wait for the third.
    fake.stats.started(3).await;
}
