//! `cleanup.inspect` while its measuring is stopped at a gate: when the cap
//! on folder removals and `workspace.changes` reads is full, and when a
//! read does not answer. Its own test binary: the cap is Core-wide, and a
//! stopped measuring holds a place of it, so it would refuse other tests'
//! reads running beside it. For the same reason its tests run one at a
//! time.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ersa_core::Gate;
use serde_json::json;
use support::clean_up::{archive, inspect, listing_with, session_in, worktrees, ws_id};
use support::put_away::{LetGo, Setup, folder, setup_with};
use support::{git, kind};

/// As many folder removals and reads as Core runs at once.
const CAP: usize = 8;

/// Held by each test for its whole run.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Eight inspects of folder items, each stopped while it measures, hold
/// every place, one each: a ninth inspect, even of a branch, which reads
/// no folder, answers `vcs_error` at once. Once they go on, an inspect is
/// answered again.
// verifies: FLOW-CLEAN-UP-R30
#[tokio::test(flavor = "multi_thread")]
async fn an_inspect_holds_one_place_and_with_none_free_answers_at_once() {
    let _alone = ONE_AT_A_TIME.lock().await;
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

/// A measuring whose read does not answer is given up on 5 seconds past its
/// limit, and stops there as when its time runs out: a `workspace` item
/// still answers, with no size, and a `folder` item, none of whose own
/// entries was read, answers `vcs_error`. Its limit is zero, so each
/// inspect waits those 5 seconds and no more.
// verifies: FLOW-CLEAN-UP-R30
// verifies: FLOW-CLEAN-UP-R31
#[tokio::test(flavor = "multi_thread")]
async fn a_measuring_whose_read_does_not_answer_stops_with_what_it_read() {
    let _alone = ONE_AT_A_TIME.lock().await;
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let stops = Arc::clone(&gate);
    let (mut s, clock) = listing_with(move |config| {
        config.measure_limit = Duration::ZERO;
        config.measure_gate = Some(stops);
    })
    .await;
    let project = s.project["id"].as_str().unwrap().to_string();
    let workspace = s.workspace().await;
    let session = session_in(&mut s, &workspace).await;
    std::fs::write(folder(&workspace).join("notes.txt"), "mine\n").unwrap();
    archive(&mut s, &clock, &session).await;
    std::fs::create_dir_all(worktrees(&s).join("stray")).unwrap();

    // Each stops at the gate, which opens only once it has answered.
    let inspected = inspect(&mut s, &ws_id(&workspace)).await;
    assert_eq!(inspected["kind"], "workspace", "{inspected}");
    assert_eq!(inspected["uncommitted"], 1, "{inspected}");
    assert!(inspected.get("size_bytes").is_none(), "{inspected}");
    gate.go();
    let id = format!("f:{project}:stray");
    let error = s.client.error("cleanup.inspect", json!({ "id": id })).await;
    assert_eq!(kind(&error), "vcs_error", "{error}");
}

/// A folder item deleted after the inspect found it, before its measuring
/// reads it, is no item now: `not_found`.
// verifies: FLOW-CLEAN-UP-R30
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_gone_before_it_is_measured_is_not_found() {
    let _alone = ONE_AT_A_TIME.lock().await;
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let stops = Arc::clone(&gate);
    let (s, _clock) = listing_with(move |config| config.measure_gate = Some(stops)).await;
    let project = s.project["id"].as_str().unwrap().to_string();
    let stray = worktrees(&s).join("stray");
    std::fs::create_dir_all(&stray).unwrap();
    std::fs::write(stray.join("a.bin"), vec![1u8; 300]).unwrap();
    let id = format!("f:{project}:stray");

    let mut client = s.core.client().await;
    let asked = client.request("cleanup.inspect", json!({ "id": id })).await;
    gate.reached().await;
    std::fs::remove_dir_all(&stray).unwrap();
    gate.go();
    let answer = client.response(asked).await;
    assert_eq!(kind(&answer["error"]), "not_found", "{answer}");
}
