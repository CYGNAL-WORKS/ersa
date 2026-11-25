//! Putting a put-away workspace back when a session there is resumed or
//! written to (FLOW-CLEAN-UP, Putting away and putting back), through the
//! Core API with real git repositories and the fake backend.

mod support;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ersa_adapter::OutgoingAttachment;
use ersa_core::Adapter;
use ersa_core::Gate;
use ersa_core::workspace::Checkout;
use serde_json::{Value, json};
use support::deleting::{FailingGit, Removal, StoppingAdds};
use support::fake::FakeAdapter;
use support::put_away::*;
use support::{git, kind, start_with_adapters, tempdir};

/// Session `id` as `session.list` lists it.
async fn session_row(s: &mut Setup, id: &str) -> Value {
    let listed = s.client.result("session.list", json!({})).await;
    let found = listed.as_array().unwrap().iter().find(|x| x["id"] == id);
    found.expect("listed").clone()
}

async fn archive(s: &mut Setup, id: &str) {
    let params = json!({ "id": id, "archived": true });
    s.client.result("session.set_archived", params).await;
}

/// How many times the fake backend was started.
fn starts(s: &Setup) -> usize {
    s.stats.starts.load(Ordering::SeqCst)
}

/// The folder the fake backend was last started in.
fn last_started_in(s: &Setup) -> String {
    s.stats.configs.lock().unwrap().last().unwrap().cwd.clone()
}

/// The branch the folder at `dir` has checked out; empty on no branch.
fn current_branch(dir: &Path) -> String {
    git(dir, &["branch", "--show-current"]).trim().to_string()
}

fn head(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// A workspace with a stopped session in it, put away.
async fn put_away_session(s: &mut Setup) -> (Value, String) {
    let workspace = s.workspace().await;
    let session = s.stopped_session(&workspace).await;
    s.put_away(&workspace).await.expect("put away");
    assert!(!folder(&workspace).exists());
    (workspace, session)
}

/// The folder is back where it was, git lists it there, and the workspace
/// is `ready`, in the store and in admission.
async fn assert_back(s: &mut Setup, workspace: &Value) {
    assert!(
        folder(workspace).join("README.md").exists(),
        "the folder is back"
    );
    assert!(listed_by_git(&s.repo, workspace), "git lists it");
    assert_eq!(s.listed(workspace).await["state"], "ready");
    assert_eq!(s.listed(workspace).await["path"], workspace["path"]);
    admitted_ready(&s.core.core(), workspace);
}

// -- Putting back (FLOW-CLEAN-UP-R03) ---------------------------------------

/// Resuming an archived session whose workspace is put away makes the
/// folder again at the same path, on the branch it had at the branch's
/// tip, which moved meanwhile, and starts the backend there; the session is
/// unarchived. A put-away workspace is not refused as `folder_gone`.
// verifies: FLOW-CLEAN-UP-R03
// verifies: FLOW-RESUME-SESSION-R23
#[tokio::test(flavor = "multi_thread")]
async fn resuming_puts_the_workspace_back_on_its_branch_at_its_tip() {
    let mut s = setup().await;
    let (workspace, session) = put_away_session(&mut s).await;
    archive(&mut s, &session).await;
    // The branch moves while the folder is away.
    let moved = commit(&s.repo, "moved.txt");
    git(&s.repo, &["branch", "-f", branch(&workspace), &moved]);

    let resumed = s
        .client
        .result("session.resume", json!({ "id": session }))
        .await;

    assert_eq!(resumed["id"], session.as_str());
    assert_back(&mut s, &workspace).await;
    let path = folder(&workspace);
    assert_eq!(current_branch(&path), branch(&workspace));
    assert_eq!(head(&path), moved, "at the branch's tip");
    assert_eq!(starts(&s), 2);
    assert_eq!(last_started_in(&s), workspace["path"].as_str().unwrap());
    assert_eq!(session_row(&mut s, &session).await["archived"], false);
    // Nothing of a put-away is stored once it is back.
    assert_eq!(s.stored(&workspace).await, None);
}

/// A folder put away on no branch comes back on the commit it was on.
// verifies: FLOW-CLEAN-UP-R03
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_on_no_branch_comes_back_on_its_commit() {
    let mut s = setup().await;
    let workspace = s.workspace().await;
    let session = s.stopped_session(&workspace).await;
    let path = folder(&workspace);
    git(&path, &["switch", "--quiet", "--detach"]);
    let detached = commit(&path, "detached.txt");
    s.put_away(&workspace).await.expect("put away");

    s.client
        .result("session.resume", json!({ "id": session }))
        .await;

    assert_back(&mut s, &workspace).await;
    assert_eq!(current_branch(&path), "", "on no branch");
    assert_eq!(head(&path), detached);
    assert_eq!(starts(&s), 2);
}

/// What a put-away left at the path goes before the folder is made again:
/// files with no record git keeps, and a worktree git still lists there
/// with changes in it.
// verifies: FLOW-CLEAN-UP-R03
#[tokio::test(flavor = "multi_thread")]
async fn what_a_put_away_left_at_the_path_is_removed_first() {
    let mut s = setup().await;

    // Files only.
    let (workspace, session) = put_away_session(&mut s).await;
    let path = folder(&workspace);
    std::fs::create_dir_all(path.join("half")).unwrap();
    std::fs::write(path.join("half").join("left.txt"), "left\n").unwrap();
    s.client
        .result("session.resume", json!({ "id": session }))
        .await;
    assert_back(&mut s, &workspace).await;
    assert!(!path.join("half").exists(), "what was left is gone");
    assert_eq!(current_branch(&path), branch(&workspace));

    // A worktree git still lists at the path, with a change in it.
    let (workspace, session) = put_away_session(&mut s).await;
    let path = folder(&workspace);
    let path_text = path.display().to_string();
    git(
        &s.repo,
        &["worktree", "add", "--quiet", "--detach", &path_text],
    );
    std::fs::write(path.join("left.txt"), "left\n").unwrap();
    s.client
        .result("session.resume", json!({ "id": session }))
        .await;
    assert_back(&mut s, &workspace).await;
    assert!(!path.join("left.txt").exists(), "what was left is gone");
    assert_eq!(current_branch(&path), branch(&workspace));
    assert_eq!(starts(&s), 4);
}

/// A message with an attachment to a session whose workspace is put away
/// puts the folder back first, then copies into it and starts the backend
/// there.
// verifies: FLOW-CLEAN-UP-R03
#[tokio::test(flavor = "multi_thread")]
async fn a_send_puts_the_workspace_back_before_it_copies() {
    let mut s = setup().await;
    let (workspace, session) = put_away_session(&mut s).await;
    let notes = s.core.dir.path().join("notes.txt");
    std::fs::write(&notes, "notes").unwrap();

    let sent = s
        .client
        .result(
            "session.send",
            json!({
                "id": session,
                "text": "again",
                "attachments": [{ "kind": "file", "path": notes.display().to_string() }],
            }),
        )
        .await;

    assert!(sent.is_object(), "{sent:#}");
    assert_back(&mut s, &workspace).await;
    assert_eq!(current_branch(&folder(&workspace)), branch(&workspace));
    // The copy is in the folder that came back: one made before would have
    // gone with what was left at the path, or stopped git making it.
    let messages = s.stats.messages.lock().unwrap().clone();
    let copied = match &messages.last().unwrap().attachments[0] {
        OutgoingAttachment::File { path } => path.clone(),
        other => panic!("not a file: {other:?}"),
    };
    // A path in the workspace, so a copy made into it.
    assert!(copied.starts_with(".ersa/"), "{copied}");
    assert!(folder(&workspace).join(&copied).exists(), "{copied}");
    assert_eq!(last_started_in(&s), workspace["path"].as_str().unwrap());
}

// -- Refused (FLOW-CLEAN-UP-R04) --------------------------------------------

/// The refusal `method` meets for `session`, with what it leaves: nothing
/// started, the workspace put away with nothing made at its path.
async fn refused(s: &mut Setup, method: &str, session: &str, workspace: &Value) -> Value {
    let before = starts(s);
    let params = match method {
        "session.send" => json!({ "id": session, "text": "again" }),
        _ => json!({ "id": session }),
    };
    let error = s.client.error(method, params).await;
    assert_eq!(kind(&error), "invalid_state", "{method}: {error:#}");
    assert_eq!(starts(s), before, "{method} started nothing");
    assert_eq!(s.listed(workspace).await["state"], "put_away", "{method}");
    assert!(!folder(workspace).exists(), "{method} made no folder");
    let state = s.core.core().admission.snapshot();
    let phase = state.workspace(id(workspace)).map(|w| w.phase);
    assert_eq!(phase, Some(ersa_core::admission::WorkspacePhase::PutAway));
    error
}

/// A branch that is gone refuses a resume and a send with `branch_gone`,
/// leaving the workspace put away and an archived session archived.
// verifies: FLOW-CLEAN-UP-R04
// verifies: PROTO-CORE-API-R43
#[tokio::test(flavor = "multi_thread")]
async fn a_branch_that_is_gone_refuses_the_resume() {
    let mut s = setup().await;
    let (workspace, session) = put_away_session(&mut s).await;
    git(&s.repo, &["branch", "-D", branch(&workspace)]);

    let error = refused(&mut s, "session.send", &session, &workspace).await;
    assert_eq!(error["data"]["reason"], "branch_gone", "{error:#}");
    archive(&mut s, &session).await;
    let error = refused(&mut s, "session.resume", &session, &workspace).await;
    assert_eq!(error["data"]["reason"], "branch_gone", "{error:#}");
    assert_eq!(session_row(&mut s, &session).await["archived"], true);
}

/// A commit that is gone refuses the same way.
// verifies: FLOW-CLEAN-UP-R04
#[tokio::test(flavor = "multi_thread")]
async fn a_commit_that_is_gone_refuses_the_resume() {
    let mut s = setup().await;
    let (workspace, session) = put_away_session(&mut s).await;
    let gone = Checkout::Commit("0123456789abcdef0123456789abcdef01234567".into());
    let core = s.core.core();
    core.store
        .put_workspace_away(id(&workspace).to_string(), gone)
        .await
        .unwrap();

    let error = refused(&mut s, "session.resume", &session, &workspace).await;
    assert_eq!(error["data"]["reason"], "branch_gone", "{error:#}");
}

/// A branch another worktree has checked out refuses a resume and a send
/// with `branch_in_use`; once that worktree lets it go, the resume puts
/// the folder back.
// verifies: FLOW-CLEAN-UP-R04
#[tokio::test(flavor = "multi_thread")]
async fn a_branch_checked_out_elsewhere_refuses_the_resume() {
    let mut s = setup().await;
    let (workspace, session) = put_away_session(&mut s).await;
    let elsewhere = s.core.dir.path().join("elsewhere");
    let elsewhere_text = elsewhere.display().to_string();
    let branch = branch(&workspace).to_string();
    git(
        &s.repo,
        &["worktree", "add", "--quiet", &elsewhere_text, &branch],
    );

    for method in ["session.resume", "session.send"] {
        let error = refused(&mut s, method, &session, &workspace).await;
        assert_eq!(error["data"]["reason"], "branch_in_use", "{error:#}");
    }

    git(&s.repo, &["worktree", "remove", &elsewhere_text]);
    s.client
        .result("session.resume", json!({ "id": session }))
        .await;
    assert_back(&mut s, &workspace).await;
}

/// git refuses to make the worktree again for a reason Core does not name,
/// as for a stored branch another worktree is rebasing, which git lists
/// there on no branch: a send and a resume answer `vcs_error` with git's
/// own words, naming the other folder, and leave the workspace put away and
/// an archived session archived.
// verifies: FLOW-CLEAN-UP-R20
#[tokio::test(flavor = "multi_thread")]
async fn a_branch_another_worktree_is_rebasing_answers_with_gits_words() {
    let mut s = setup().await;
    let workspace = s.workspace().await;
    let session = s.stopped_session(&workspace).await;
    // The branch and `main` change one file each their own way, so a
    // rebase of the branch onto `main` stops on the conflict.
    let path = folder(&workspace);
    std::fs::write(path.join("README.md"), "the branch's\n").unwrap();
    git(&path, &["commit", "--quiet", "-am", "the branch's"]);
    s.put_away(&workspace).await.expect("put away");
    std::fs::write(s.repo.join("README.md"), "main's\n").unwrap();
    git(&s.repo, &["commit", "--quiet", "-am", "main's"]);
    let elsewhere = s.core.dir.path().join("rebasing-elsewhere");
    let elsewhere_text = elsewhere.display().to_string();
    git(
        &s.repo,
        &["worktree", "add", "--quiet", "--detach", &elsewhere_text],
    );
    // Sealed as `git` is, but not `git`: the rebase stopping is the point,
    // and that helper asserts success.
    let rebase = support::sealed::git_command(&elsewhere)
        .args(["rebase", "main", branch(&workspace)])
        .output()
        .expect("git runs");
    assert!(!rebase.status.success(), "the rebase stops on the conflict");

    for method in ["session.send", "session.resume"] {
        if method == "session.resume" {
            archive(&mut s, &session).await;
        }
        let params = match method {
            "session.send" => json!({ "id": session, "text": "again" }),
            _ => json!({ "id": session }),
        };
        let error = s.client.error(method, params).await;
        assert_eq!(kind(&error), "vcs_error", "{method}: {error:#}");
        let detail = error["data"]["detail"].as_str().unwrap_or_default();
        assert!(detail.contains("rebasing-elsewhere"), "{method}: {error:#}");
        assert_eq!(starts(&s), 1, "{method} started nothing");
        assert_eq!(s.listed(&workspace).await["state"], "put_away", "{method}");
    }
    assert_eq!(session_row(&mut s, &session).await["archived"], true);
}

// -- What a put-back holds (PROTO-CORE-API-R143, R144) ---------------------

/// A Core whose every `git worktree add` stops at `gate`; its transport,
/// which notes each change to worktrees.
async fn stopping_adds(gate: &Arc<Gate>) -> (Setup, Arc<StoppingAdds>) {
    let git = Arc::new(StoppingAdds::new(gate));
    let transport = Arc::clone(&git);
    let s = setup_with(move |config| {
        config.command_transport = Some(ersa_core::CommandTransport(transport));
    })
    .await;
    (s, git)
}

/// A workspace made through `gate`.
async fn workspace_through(s: &mut Setup, gate: &Gate) -> Value {
    let params = json!({ "project_id": s.project["id"] });
    let making = s.client.request("workspace.create", params).await;
    reached(gate).await;
    gate.go();
    s.client.response(making).await["result"].clone()
}

/// Forking, deleting and removing `session` answer `busy` while its
/// workspace is held, and removing the workspace answers `invalid_state`.
async fn assert_held(s: &mut Setup, session: &str, workspace: &Value) {
    let changes = json!({ "id": workspace["id"] });
    let token = s.client.result("workspace.changes", changes).await["token"].clone();
    for (method, params) in [
        ("session.fork", json!({ "id": session })),
        ("session.delete", json!({ "id": session, "token": token })),
        ("session.remove", json!({ "id": session })),
    ] {
        let error = s.client.error(method, params).await;
        assert_eq!(kind(&error), "invalid_state", "{method}: {error:#}");
        assert_eq!(error["data"]["detail"], "busy", "{method}: {error:#}");
    }
    let params = json!({ "id": workspace["id"], "force": true });
    let error = s.client.error("workspace.remove", params).await;
    assert_eq!(kind(&error), "invalid_state", "{error:#}");
}

/// While one session's resume puts the folder back, a fork, a delete or a
/// removal of another session there answers `busy` and removing the
/// workspace `invalid_state`; the other session's resume waits for the
/// put-back and then puts nothing back.
// verifies: PROTO-CORE-API-R143
#[tokio::test(flavor = "multi_thread")]
async fn a_put_back_under_way_holds_its_workspace() {
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let (mut s, git) = stopping_adds(&gate).await;
    let workspace = workspace_through(&mut s, &gate).await;
    let first = s.stopped_session(&workspace).await;
    let second = s.stopped_session(&workspace).await;
    s.put_away(&workspace).await.expect("put away");
    let resuming = s
        .client
        .request("session.resume", json!({ "id": first }))
        .await;
    // The put-back's add, stopped.
    reached(&gate).await;

    assert_held(&mut s, &second, &workspace).await;
    assert_eq!(s.listed(&workspace).await["state"], "put_away");
    let waiting = s
        .client
        .request("session.resume", json!({ "id": second }))
        .await;
    let core = s.core.core();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !core
        .admission
        .snapshot()
        .held()
        .iter()
        .any(|held| matches!(held, ersa_core::admission::Command::PutBack { .. }))
    {
        assert!(tokio::time::Instant::now() < deadline, "never waited");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    gate.go();

    assert!(s.client.response(resuming).await["result"].is_object());
    // A second add would stop at the gate, and the answer never come.
    let second_answer = tokio::time::timeout(Duration::from_secs(30), s.client.response(waiting));
    let second_answer = second_answer.await.expect("answered without a second add");
    assert!(second_answer["result"].is_object(), "{second_answer:#}");
    assert_back(&mut s, &workspace).await;
    let adds = git.changes().iter().filter(|c| *c == "add").count();
    assert_eq!(adds, 2, "made once, put back once");
    assert_eq!(starts(&s), 4);
}

/// A try at the folder that a put-back gave up on holds the workspace too,
/// until it ends.
// verifies: PROTO-CORE-API-R143
#[tokio::test(flavor = "multi_thread")]
async fn a_try_a_put_back_gave_up_on_holds_its_workspace() {
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let mut s = stuck_folder_setup(&gate, ersa_core::workspace::WORKTREE_TURN_WAIT).await;
    let workspace = s.workspace().await;
    let first = s.stopped_session(&workspace).await;
    let second = s.stopped_session(&workspace).await;
    let core = s.core.core();
    // Put away, its try at the folder let go however long it waited.
    let putting = tokio::spawn({
        let core = Arc::clone(&core);
        let id = id(&workspace).to_string();
        async move { ersa_core::api::put_away_workspace(&core, &id).await }
    });
    reached(&gate).await;
    gate.go();
    let _ = putting.await.unwrap();
    put_away_once_held(&core, &workspace).await;
    // Files a put-away left at the path: the put-back's try at them stops
    // at the gate and is given up on.
    let path = folder(&workspace);
    std::fs::create_dir_all(path.join("half")).unwrap();
    std::fs::write(path.join("half").join("left.txt"), "left\n").unwrap();
    let resuming = s
        .client
        .request("session.resume", json!({ "id": first }))
        .await;
    reached(&gate).await;
    let answer = s.client.response(resuming).await;
    assert_eq!(kind(&answer["error"]), "vcs_error", "{answer:#}");

    assert_held(&mut s, &second, &workspace).await;
    assert_eq!(s.listed(&workspace).await["state"], "put_away");

    // The try returns; then the hold goes, and admission has the workspace
    // put away, which nothing holds.
    gate.go();
    put_away_once_held(&core, &workspace).await;
}

/// While a delete of one session holds a put-away workspace, a resume of
/// another session there, or a send to it, answers `busy` and puts nothing
/// back; once the delete ends, the resume puts the folder back.
// verifies: PROTO-CORE-API-R144
#[tokio::test(flavor = "multi_thread")]
async fn nothing_is_put_back_while_a_delete_holds_the_workspace() {
    let fake = Arc::new(FakeAdapter::new(1));
    let stats = Arc::clone(&fake.stats);
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::clone(&fake) as Arc<dyn Adapter>];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let mut s = Setup::over(core, stats).await;
    let workspace = s.workspace().await;
    let first = s.stopped_session(&workspace).await;
    let second = s.stopped_session(&workspace).await;
    s.put_away(&workspace).await.expect("put away");
    // The delete holds the workspace while its record deletion stalls.
    let stalled = fake.delete_gate.lock().await;
    let changes = json!({ "id": workspace["id"] });
    let token = s.client.result("workspace.changes", changes).await["token"].clone();
    let params = json!({ "id": first, "token": token });
    let deleting = s.client.request("session.delete", params).await;
    let core = s.core.core();
    let held = || {
        let state = core.admission.snapshot();
        let entry = state.workspace(id(&workspace)).cloned();
        entry.is_some_and(|w| w.deleting.is_some())
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !held() {
        assert!(tokio::time::Instant::now() < deadline, "never held");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    for method in ["session.resume", "session.send"] {
        // `refused` asserts nothing was put back: no folder, still put away.
        let error = refused(&mut s, method, &second, &workspace).await;
        assert_eq!(error["data"]["detail"], "busy", "{method}: {error:#}");
    }

    drop(stalled);
    assert!(s.client.response(deleting).await["result"].is_object());
    s.client
        .result("session.resume", json!({ "id": second }))
        .await;
    assert_back(&mut s, &workspace).await;
}

// -- Waiting (PROTO-CORE-API-R124, R132, R134) ------------------------------

/// A Core whose git stops each worktree's first removal at `gate`, then
/// drops git's record of it and fails, so the put-away goes on to the folder
/// and finishes; with `wait` for each wait at the project's worktrees.
async fn stopping_put_away(gate: &Arc<Gate>, wait: Duration) -> Setup {
    let git =
        FailingGit::new(Removal::DropsRecordThenFails, None).stopping_removals(Arc::clone(gate));
    let git = Arc::new(git);
    setup_with(move |config| {
        config.command_transport = Some(ersa_core::CommandTransport(git));
        config.worktree_turn_wait = wait;
    })
    .await
}

/// Starts putting `workspace` away, and returns once its removal is
/// stopped at `gate`, the workspace held by the put-away.
async fn put_away_under_way(s: &Setup, gate: &Gate, workspace: &Value) {
    let core = s.core.core();
    let id = id(workspace).to_string();
    tokio::spawn(async move { ersa_core::api::put_away_workspace(&core, &id).await });
    reached(gate).await;
}

/// A resume and a send that find their workspace being put away wait for
/// that to end and then put it back, rather than being refused; the send
/// says meanwhile that it is putting the folder back.
// verifies: PROTO-CORE-API-R132
// verifies: PROTO-CORE-API-R134
// verifies: FLOW-CLEAN-UP-R05
#[tokio::test(flavor = "multi_thread")]
async fn a_resume_or_send_during_a_put_away_waits_for_it() {
    for method in ["session.send", "session.resume"] {
        let gate = Arc::new(Gate::default());
        let _let_go = LetGo(Arc::clone(&gate));
        let mut s = stopping_put_away(&gate, Duration::from_secs(300)).await;
        let workspace = s.workspace().await;
        let session = s.stopped_session(&workspace).await;
        put_away_under_way(&s, &gate, &workspace).await;
        s.client.clear_notifications();

        let params = json!({ "id": session, "text": "again" });
        let params = match method {
            "session.send" => params,
            _ => json!({ "id": session }),
        };
        let asked = s.client.request(method, params).await;
        if method == "session.send" {
            let report = s
                .client
                .notification_where(|n| {
                    n["method"] == "session.send_progress" && n["params"]["phase"] == "putting_back"
                })
                .await;
            assert_eq!(report["params"]["session_id"], session.as_str());
            // `session.list` says so too.
            let row = session_row(&mut s, &session).await;
            assert_eq!(row["sending"], "putting_back", "{row:#}");
        }
        assert_eq!(starts(&s), 1, "{method}: nothing starts meanwhile");

        gate.go();
        let answer = s.client.response(asked).await;
        assert!(answer["result"].is_object(), "{method}: {answer:#}");
        assert_back(&mut s, &workspace).await;
        assert_eq!(starts(&s), 2, "{method}");
    }
}

/// A put-away that has not ended within the put-back's wait answers the
/// resume `vcs_error`, putting nothing back.
// verifies: PROTO-CORE-API-R132
#[tokio::test(flavor = "multi_thread")]
async fn a_put_away_that_outlasts_the_wait_answers_vcs_error() {
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let mut s = stopping_put_away(&gate, Duration::from_millis(300)).await;
    let workspace = s.workspace().await;
    let session = s.stopped_session(&workspace).await;
    put_away_under_way(&s, &gate, &workspace).await;

    let error = s
        .client
        .error("session.resume", json!({ "id": session }))
        .await;

    assert_eq!(kind(&error), "vcs_error", "{error:#}");
    assert_eq!(starts(&s), 1);
    assert_eq!(s.listed(&workspace).await["state"], "put_away");
    // Once the put-away ends, the resume puts the folder back.
    gate.go();
    put_away_once_held(&s.core.core(), &workspace).await;
    s.client
        .result("session.resume", json!({ "id": session }))
        .await;
    assert_back(&mut s, &workspace).await;
}

/// A put-back waits its turn at the project's worktrees; past the wait it
/// begins no git work and answers `vcs_error`, the workspace put away.
// verifies: PROTO-CORE-API-R124
#[tokio::test(flavor = "multi_thread")]
async fn a_put_back_that_waited_too_long_for_its_turn_puts_nothing_back() {
    let mut s = setup_with(|config| config.worktree_turn_wait = Duration::from_millis(300)).await;
    let (workspace, session) = put_away_session(&mut s).await;
    let core = s.core.core();
    let project = s.project["id"].as_str().unwrap().to_string();
    let turn = core
        .admission
        .worktrees(&project, Duration::from_secs(60))
        .await
        .expect("the project's turn");

    let error = s
        .client
        .error("session.resume", json!({ "id": session }))
        .await;

    assert_eq!(kind(&error), "vcs_error", "{error:#}");
    assert!(!folder(&workspace).exists(), "no git work began");
    assert!(!listed_by_git(&s.repo, &workspace));
    assert_eq!(s.listed(&workspace).await["state"], "put_away");
    assert_eq!(starts(&s), 1);

    drop(turn);
    s.client
        .result("session.resume", json!({ "id": session }))
        .await;
    assert_back(&mut s, &workspace).await;
}

/// The wait for a put-away and the put-back's wait for its turn at the
/// project's worktrees share one limit: a resume that spent part of it
/// waiting for a try at the folder that the put-away gave up on, and then
/// finds another change to the worktrees under way, answers `vcs_error`
/// once the two waits together reach the limit, not a whole limit after
/// the try ended.
// verifies: PROTO-CORE-API-R132
#[tokio::test(flavor = "multi_thread")]
async fn the_put_away_and_the_turn_share_one_wait() {
    const WAIT: Duration = Duration::from_secs(6);
    // How long the stuck try holds the resume: part of `WAIT`.
    const HELD: Duration = Duration::from_secs(4);
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let mut s = stuck_folder_setup(&gate, WAIT).await;
    let workspace = s.workspace().await;
    let session = s.stopped_session(&workspace).await;
    // The put-away gives up on its try at the folder, which stays stuck at
    // the gate holding the workspace; nothing holds the project's turn.
    let core = s.core.core();
    let putting = tokio::spawn({
        let core = Arc::clone(&core);
        let id = id(&workspace).to_string();
        async move { ersa_core::api::put_away_workspace(&core, &id).await }
    });
    reached(&gate).await;
    assert!(putting.await.unwrap().is_err(), "the try ran out");
    // Another change to the project's worktrees has the turn throughout.
    let project = s.project["id"].as_str().unwrap().to_string();
    let _turn = core
        .admission
        .worktrees(&project, Duration::from_secs(60))
        .await
        .expect("the project's turn");

    let began = std::time::Instant::now();
    let asked = s
        .client
        .request("session.resume", json!({ "id": session }))
        .await;
    // The scenario, not a pause sized to pass: the stuck try holds the
    // resume for this part of the wait, then returns.
    tokio::time::sleep(HELD).await;
    gate.go();
    let answer = s.client.response(asked).await;
    let waited = began.elapsed();

    assert_eq!(kind(&answer["error"]), "vcs_error", "{answer:#}");
    // Turned away for the turn, not for the put-away it waited out.
    let message = answer["error"]["message"].as_str().unwrap_or_default();
    assert!(!message.contains("still being put"), "{answer:#}");
    // One limit in all, about `WAIT`; two would be `HELD + WAIT`.
    assert!(waited >= HELD, "answered after {waited:?}");
    assert!(
        waited < WAIT + (HELD / 2),
        "answered after {waited:?}, past one wait of {WAIT:?}"
    );
    assert_eq!(s.listed(&workspace).await["state"], "put_away");
    assert_eq!(starts(&s), 1);
}

/// Stop while a send waits for its workspace's put-away answers at once
/// that the message was stopped; the put-back goes on to its end, and the
/// backend is not started.
// verifies: PROTO-CORE-API-R145
#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_send_that_waits_for_a_put_back_answers_at_once() {
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let mut s = stopping_put_away(&gate, Duration::from_secs(300)).await;
    let workspace = s.workspace().await;
    let session = s.stopped_session(&workspace).await;
    put_away_under_way(&s, &gate, &workspace).await;
    s.client.clear_notifications();
    let params = json!({ "id": session, "text": "again" });
    let sending = s.client.request("session.send", params).await;
    s.client
        .notification_where(|n| n["params"]["phase"] == "putting_back")
        .await;

    s.client
        .result("session.interrupt", json!({ "id": session }))
        .await;
    let answer = s.client.response(sending).await;

    assert_eq!(
        answer["error"]["data"]["reason"], "send_stopped",
        "{answer:#}"
    );
    gate.go();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while s.listed(&workspace).await["state"] != "ready" {
        assert!(tokio::time::Instant::now() < deadline, "never put back");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_back(&mut s, &workspace).await;
    assert_eq!(starts(&s), 1, "the backend was not started");
    assert_eq!(session_row(&mut s, &session).await["state"], "ended");
}

/// Stop while a send's put-back is making the folder answers at once that
/// the message was stopped; the put-back goes on to make the folder, the
/// workspace is then ready, and no backend starts.
// verifies: PROTO-CORE-API-R145
#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_send_while_its_folder_is_put_back_answers_at_once() {
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let (mut s, _git) = stopping_adds(&gate).await;
    let workspace = workspace_through(&mut s, &gate).await;
    let session = s.stopped_session(&workspace).await;
    s.put_away(&workspace).await.expect("put away");
    let params = json!({ "id": session, "text": "again" });
    let sending = s.client.request("session.send", params).await;
    // The put-back's add, stopped.
    reached(&gate).await;

    s.client
        .result("session.interrupt", json!({ "id": session }))
        .await;
    let answer = s.client.response(sending).await;

    assert_eq!(
        answer["error"]["data"]["reason"], "send_stopped",
        "{answer:#}"
    );
    gate.go();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while s.listed(&workspace).await["state"] != "ready" {
        assert!(tokio::time::Instant::now() < deadline, "never put back");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_back(&mut s, &workspace).await;
    assert_eq!(starts(&s), 1, "the backend was not started");
    assert_eq!(session_row(&mut s, &session).await["state"], "ended");
}
