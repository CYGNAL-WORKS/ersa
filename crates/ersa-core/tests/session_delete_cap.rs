//! `session.delete` when the cap on folder removals and `workspace.changes`
//! reads is full. Its own test binary: the cap is Core-wide, so filling it
//! would refuse other tests' reads running beside it.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ersa_core::{Adapter, CommandTransport, Gate};
use serde_json::json;
use support::deleting::{FailingGit, Removal, idle_session, isolated, listed, token};
use support::fake::FakeAdapter;
use support::{git_repo, kind, start_with_adapters, tempdir};

/// As many folder removals and reads as Core runs at once.
const CAP: usize = 8;

/// Lets every stop go however the test ends, so no try is left parked.
struct LetGo(Vec<Arc<Gate>>);

impl Drop for LetGo {
    fn drop(&mut self) {
        for gate in &self.0 {
            for _ in 0..=CAP {
                gate.go();
            }
        }
    }
}

/// Eight other deletes' tries at their folders are stuck past their limit,
/// each keeping its place under the cap until it returns. A ninth delete's
/// try cannot begin: the delete fails at once with `vcs_error`, the session
/// reads failed, and no try at its folder begins.
// verifies: PROTO-CORE-API-R116
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_try_that_cannot_begin_fails_the_delete_at_once() {
    let fake = Arc::new(FakeAdapter::new(1));
    // Where each delete stops past its step 3, and where each try at a
    // folder stops: a folder that does not answer.
    let (removals, folders) = (Arc::new(Gate::default()), Arc::new(Gate::default()));
    let _let_go = LetGo(vec![Arc::clone(&removals), Arc::clone(&folders)]);
    // git drops its record and fails, so each delete goes on to its folder.
    let git = Arc::new(
        FailingGit::new(Removal::DropsRecordThenFails, None)
            .stopping_removals(Arc::clone(&removals)),
    );
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::clone(&fake) as Arc<dyn Adapter>];
    let gate = Arc::clone(&folders);
    let core = start_with_adapters(
        tempdir(),
        move |config| {
            config.command_transport = Some(CommandTransport(git));
            config.folder_gate = Some(gate);
            config.folder_removal_limit = Duration::from_millis(200);
        },
        adapters,
    )
    .await;
    let mut client = core.client().await;
    // The project's inspection stops at the folder gate too: let it go.
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let params =
        json!({ "host_id": "local", "path": repo.display().to_string(), "trusted": false });
    let add = client.request("project.add", params).await;
    reached(&folders).await;
    folders.go();
    let project = client.response(add).await["result"].clone();

    // Nine sessions, each alone in a workspace, set up side by side.
    let mut setups = Vec::new();
    for _ in 0..=CAP {
        let mut client = core.client().await;
        let project = project.clone();
        setups.push(tokio::spawn(async move {
            let (workspace, folder) = isolated(&mut client, &project).await;
            let id = idle_session(&mut client, &workspace).await;
            let shown = token(&mut client, &workspace).await;
            (id, folder, shown)
        }));
    }
    // Their deletes, sent one after another, each stopped past its step 3:
    // the last is the one under test.
    let mut deletes = Vec::new();
    for setup in setups {
        let (id, folder, shown) = setup.await.unwrap();
        let request = client
            .request("session.delete", json!({ "id": id, "token": shown }))
            .await;
        reached(&removals).await;
        deletes.push((id, folder, request));
    }
    let (id, folder, request) = deletes.pop().unwrap();

    // Fill the cap: the eight others, let go in the order they stopped, go
    // on to their folders, where each try gets stuck and is answered at its
    // limit, keeping its place.
    for _ in &deletes {
        removals.go();
    }
    for _ in &deletes {
        reached(&folders).await;
    }
    for (_, _, other) in deletes {
        let answer = client.response(other).await;
        let message = answer["error"]["message"].as_str().unwrap_or_default();
        assert!(message.contains("did not finish"), "{answer}");
    }

    removals.go();
    let answer = tokio::time::timeout(Duration::from_secs(10), client.response(request))
        .await
        .expect("answered at once, not after 30 seconds of tries");
    assert_eq!(kind(&answer["error"]), "vcs_error", "{answer}");
    let message = answer["error"]["message"].as_str().unwrap();
    assert!(message.contains("too many"), "{answer}");
    // No try at its folder began.
    let began = tokio::time::timeout(Duration::from_millis(500), folders.reached()).await;
    assert!(began.is_err(), "a try at the folder began");
    assert!(folder.is_dir());
    assert_eq!(
        listed(&mut client, &id).await.unwrap()["deleting"],
        "failed"
    );
}

/// Waits until a request stops at `gate`, failing instead of hanging.
async fn reached(gate: &Gate) {
    tokio::time::timeout(Duration::from_secs(30), gate.reached())
        .await
        .expect("the request stopped in time");
}
