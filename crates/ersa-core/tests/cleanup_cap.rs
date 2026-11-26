//! `cleanup.inspect` when the cap on folder removals and `workspace.changes`
//! reads is full. Its own test binary: the cap is Core-wide, so filling it
//! would refuse other tests' reads running beside it.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ersa_core::Gate;
use serde_json::json;
use support::put_away::{LetGo, Setup, setup_with};
use support::{git, kind};

/// As many folder removals and reads as Core runs at once.
const CAP: usize = 8;

/// Eight inspects of folder items, each stopped while it measures, hold
/// every place, one each: a ninth inspect, even of a branch, which reads
/// no folder, answers `vcs_error` at once. Once they go on, an inspect is
/// answered again.
// verifies: FLOW-CLEAN-UP-R12
#[tokio::test(flavor = "multi_thread")]
async fn an_inspect_holds_one_place_and_with_none_free_answers_at_once() {
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let stops = Arc::clone(&gate);
    let mut s: Setup = setup_with(move |config| config.measure_gate = Some(stops)).await;
    let project = s.project["id"].as_str().unwrap().to_string();
    let name = s.project["name"].as_str().unwrap().to_string();
    let here = s.core.dir.path().join("worktrees").join(&name);
    git(&s.repo, &["branch", "ersa/stray"]);

    let mut clients = Vec::new();
    for n in 0..CAP {
        std::fs::create_dir_all(here.join(format!("stray-{n}"))).unwrap();
    }
    for n in 0..CAP {
        let mut client = s.core.client().await;
        let id = format!("f:{project}:stray-{n}");
        let asked = client.request("cleanup.inspect", json!({ "id": id })).await;
        gate.reached().await;
        clients.push((client, asked));
    }

    let branch = json!({ "id": format!("b:{project}:ersa/stray") });
    let error = tokio::time::timeout(
        Duration::from_secs(10),
        s.client.error("cleanup.inspect", branch.clone()),
    )
    .await
    .expect("answered at once");
    assert_eq!(kind(&error), "vcs_error", "{error}");
    // At once: before it reads anything, even what the id names.
    let nothing = json!({ "id": "b:p-none:ersa/none" });
    let error = s.client.error("cleanup.inspect", nothing).await;
    assert_eq!(kind(&error), "vcs_error", "{error}");

    for _ in 0..CAP {
        gate.go();
    }
    for (mut client, asked) in clients {
        let answer = client.response(asked).await;
        assert_eq!(answer["result"]["size_bytes"], 0, "{answer}");
    }
    let answer = s.client.result("cleanup.inspect", branch).await;
    assert_eq!(answer["merged"], true, "{answer}");
}
