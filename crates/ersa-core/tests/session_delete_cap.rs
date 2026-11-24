//! `session.delete` when the cap on folder removals and `workspace.changes`
//! reads is full. Its own test binary: the cap is Core-wide, so filling it
//! would refuse other tests' reads running beside it.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ersa_core::{Adapter, CommandTransport, Gate};
use serde_json::json;
use support::deleting::{FailingGit, Removal, idle_session, isolated, token};
use support::fake::FakeAdapter;
use support::listed;
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

/// Nine deletes in one repository, each past its step 4. Their git
/// removals take the repository's turn one at a time; the first eight go on
/// to their folders, where each try gets stuck past its limit and keeps its
/// place under the cap until it returns. The ninth's try cannot begin: that
/// delete fails at once with `vcs_error`, the session reads failed, and no
/// try at its folder begins.
// verifies: PROTO-CORE-API-R116
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_try_that_cannot_begin_fails_the_delete_at_once() {
    let fake = Arc::new(FakeAdapter::new(1));
    // Where each delete's git removal stops, holding the repository's
    // turn, and where each try at a folder stops: a folder that does not
    // answer.
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

    // Nine sessions, each alone in a workspace, set up side by side: nine
    // `git worktree add`s in one repository at once.
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
    let mut deletes = Vec::new();
    for setup in setups {
        let (id, folder, shown) = setup.await.unwrap();
        let request = client
            .request("session.delete", json!({ "id": id, "token": shown }))
            .await;
        deletes.push((id, folder, request));
    }
    // One removal holds the repository's turn at its stop; the others wait
    // for it past step 4, their reads of step 3 done before the cap fills.
    reached(&removals).await;
    for (id, _, _) in &deletes {
        running(&mut client, id).await;
    }

    // Fill the cap: eight removals, let go one at a time, go on to their
    // folders, where each try gets stuck and is answered at its limit,
    // keeping its place.
    for _ in 0..CAP {
        removals.go();
        reached(&folders).await;
        reached(&removals).await;
    }
    // The ninth, last to its folder, is the one under test.
    removals.go();
    let mut stuck = 0;
    let mut refused = Vec::new();
    for (id, folder, request) in deletes {
        let answer = tokio::time::timeout(Duration::from_secs(10), client.response(request))
            .await
            .expect("answered at once, not after 30 seconds of tries");
        let message = answer["error"]["message"].as_str().unwrap_or_default();
        if message.contains("did not finish") {
            stuck += 1;
        } else {
            assert_eq!(kind(&answer["error"]), "vcs_error", "{answer}");
            assert!(message.contains("too many"), "{answer}");
            refused.push((id, folder));
        }
    }
    assert_eq!((stuck, refused.len()), (CAP, 1));
    let (id, folder) = refused.pop().unwrap();
    // No try at its folder began.
    let began = tokio::time::timeout(Duration::from_millis(500), folders.reached()).await;
    assert!(began.is_err(), "a try at the folder began");
    assert!(folder.is_dir());
    assert_eq!(
        listed(&mut client, &id).await.unwrap()["deleting"],
        "failed"
    );
}

/// Waits until session `id`'s delete is past its step 4: it reads
/// `deleting: running`.
async fn running(client: &mut support::Client, id: &str) {
    let marked = async {
        loop {
            let session = listed(client, id).await.unwrap();
            if session["deleting"] == "running" {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(30), marked)
        .await
        .expect("the delete was past its step 4 in time");
}

/// Waits until a request stops at `gate`, failing instead of hanging.
async fn reached(gate: &Gate) {
    tokio::time::timeout(Duration::from_secs(30), gate.reached())
        .await
        .expect("the request stopped in time");
}
