//! Removing one Clean up item (FLOW-CLEAN-UP-R13, R14, R27): `cleanup.remove`
//! over the socket, against real git repositories and the fake backend. Each
//! test inspects the item first, as the Interface does, and removes it with
//! the token it was shown.

mod support;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ersa_core::Gate;
use serde_json::{Value, json};
use support::clean_up::*;
use support::put_away::*;
use support::{assert_valid, git, kind};

/// Core's blocking file system jobs share one limit, Core-wide (eight places,
/// PROTO-CORE-API-R116), and answer `vcs_error` past it. Every test here
/// starts a Core of its own in this one process, and each inspects, counts
/// and removes, so at most two run at once and none meets another's jobs.
static FEW_AT_A_TIME: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

/// Waits for a turn among [`FEW_AT_A_TIME`]; the test keeps it to its end.
async fn turn() -> tokio::sync::SemaphorePermit<'static> {
    FEW_AT_A_TIME.acquire().await.expect("never closed")
}

/// `cleanup.remove`, checked against the schema.
async fn remove(s: &mut Setup, id: &str, token: &Value, discard: Option<bool>) -> Value {
    let mut params = json!({ "id": id, "token": token });
    if let Some(discard) = discard {
        params["discard"] = json!(discard);
    }
    let response = s.client.call("cleanup.remove", params).await;
    assert_valid("cleanup.remove", &response);
    response
}

/// Inspects `id`, then removes it with the token it was shown.
async fn inspect_and_remove(s: &mut Setup, id: &str, discard: Option<bool>) -> Value {
    let token = inspect(s, id).await["token"].clone();
    remove(s, id, &token, discard).await
}

fn result(response: &Value) -> &Value {
    assert!(response.get("result").is_some(), "{response}");
    &response["result"]
}

fn error(response: &Value) -> &Value {
    assert!(response.get("error").is_some(), "{response}");
    &response["error"]
}

/// The error's `data.reason`, if any.
fn reason(error: &Value) -> Option<&str> {
    error["data"]["reason"].as_str()
}

/// The error's `data.detail`, read as the JSON it holds.
fn detail(error: &Value) -> Value {
    let text = error["data"]["detail"].as_str().expect("a detail");
    serde_json::from_str(text).expect("the detail is JSON")
}

/// Refused as R13 refuses an item whose removal would lose unsaved work
/// without `discard`: `invalid_state`, no reason, no detail.
fn refused_for_unsaved_work(response: &Value) {
    let error = error(response);
    assert_eq!(kind(error), "invalid_state", "{error}");
    assert!(reason(error).is_none(), "{error}");
    assert!(error["data"].get("detail").is_none(), "{error}");
}

/// A session of the fake backend in a new workspace, with a turn behind it,
/// stopped, unsubscribed and archived; and the workspace, still `ready`
/// with nothing in it to lose: a `workspace` item.
async fn archived_workspace(s: &mut Setup, clock: &ersa_core::SweepClock) -> (Value, String) {
    let workspace = s.workspace().await;
    let session = session_in(s, &workspace).await;
    // Kept by the sweep the archive asks for, for this file; then gone.
    let keep = folder(&workspace).join("keep.txt");
    std::fs::write(&keep, "keep\n").unwrap();
    archive(s, clock, &session).await;
    std::fs::remove_file(keep).unwrap();
    assert_eq!(s.listed(&workspace).await["state"], "ready");
    (workspace, session)
}

/// Session `id` as `session.list` lists it, if it does.
async fn session_row(s: &mut Setup, id: &str) -> Option<Value> {
    let listed = s.client.result("session.list", json!({})).await;
    listed
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == id)
        .cloned()
}

fn head(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// A workspace no session refers to, made more than an hour ago by the
/// sweep's clock: a `leftover` item.
async fn leftover(s: &mut Setup, clock: &ersa_core::SweepClock) -> Value {
    let workspace = s.workspace().await;
    clock.time_passed(HOUR + Duration::from_secs(60));
    workspace
}

// -- Workspace items: put away (FLOW-CLEAN-UP-R13) --------------------------

/// A `workspace` item is put away: its folder and git's record go, and the
/// workspace, its sessions, their records and every branch stay; the
/// answer names the branch kept.
// verifies: FLOW-CLEAN-UP-R13
// verifies: FLOW-CLEAN-UP-R14
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_item_is_put_away_keeping_its_sessions_and_branch() {
    let _turn = turn().await;
    let (mut s, clock) = listing().await;
    let (workspace, session) = archived_workspace(&mut s, &clock).await;
    // A commit of its own on its branch, which the branch keeps.
    commit(&folder(&workspace), "notes.txt");
    let deletes = s.stats.record_deletes.load(Ordering::SeqCst);

    let response = inspect_and_remove(&mut s, &ws_id(&workspace), None).await;

    let answer = result(&response);
    assert_eq!(answer["id"], ws_id(&workspace));
    assert_eq!(answer["branch_kept"], workspace["vcs_ref"]);
    assert!(!folder(&workspace).exists(), "the folder is gone");
    assert!(!listed_by_git(&s.repo, &workspace), "git's record is gone");
    assert!(has_branch(&s.repo, branch(&workspace)), "the branch stays");
    assert_eq!(s.listed(&workspace).await["state"], "put_away");
    let row = session_row(&mut s, &session).await.expect("still listed");
    assert_eq!(row["archived"], true);
    assert_eq!(row["workspace_id"], workspace["id"]);
    assert_eq!(
        s.stats.record_deletes.load(Ordering::SeqCst),
        deletes,
        "no session record deleted"
    );
}

/// A session whose folder `cleanup.remove` put away resumes with its folder
/// back, on its branch, whether it was archived or only inactive.
// verifies: FLOW-CLEAN-UP-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_session_whose_folder_was_put_away_resumes_with_it_back() {
    let _turn = turn().await;
    let (mut s, clock) = listing().await;
    let (archived, first) = archived_workspace(&mut s, &clock).await;
    let inactive = s.workspace().await;
    let second = session_in(&mut s, &inactive).await;
    clock.time_passed(31 * DAY);
    let tip = head(&folder(&inactive));

    for workspace in [&archived, &inactive] {
        let response = inspect_and_remove(&mut s, &ws_id(workspace), None).await;
        result(&response);
        assert!(!folder(workspace).exists());
    }
    // The inactive one was not archived by its folder going.
    let row = session_row(&mut s, &second).await.unwrap();
    assert_eq!(row["archived"], false);

    for (workspace, session) in [(&archived, &first), (&inactive, &second)] {
        s.client
            .result("session.resume", json!({ "id": session }))
            .await;
        assert!(folder(workspace).join("README.md").exists(), "back");
        assert!(listed_by_git(&s.repo, workspace));
        let on = git(&folder(workspace), &["branch", "--show-current"]);
        assert_eq!(on.trim(), branch(workspace));
        assert_eq!(s.listed(workspace).await["state"], "ready");
        let row = session_row(&mut s, session).await.unwrap();
        assert_eq!(row["archived"], false);
    }
    assert_eq!(head(&folder(&inactive)), tip);
}

/// A `workspace` item with uncommitted files, or with commits only its
/// folder's checkout holds, is refused without `discard` and nothing goes;
/// with `discard` it is put away and they go with the folder.
// verifies: FLOW-CLEAN-UP-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_item_holding_unsaved_work_goes_only_with_discard() {
    let _turn = turn().await;
    let (mut s, clock) = listing().await;
    let (dirty, _) = archived_workspace(&mut s, &clock).await;
    std::fs::write(folder(&dirty).join("notes.txt"), "mine\n").unwrap();
    let (stranded, _) = archived_workspace(&mut s, &clock).await;
    git(&folder(&stranded), &["switch", "--quiet", "--detach"]);
    commit(&folder(&stranded), "only-here.txt");
    let inspected = inspect(&mut s, &ws_id(&stranded)).await;
    assert_eq!(inspected["stranded"], 1, "{inspected}");

    for workspace in [&dirty, &stranded] {
        let id = ws_id(workspace);
        let refused = inspect_and_remove(&mut s, &id, None).await;
        refused_for_unsaved_work(&refused);
        let refused = inspect_and_remove(&mut s, &id, Some(false)).await;
        refused_for_unsaved_work(&refused);
        assert!(folder(workspace).exists(), "nothing removed");
        assert_eq!(s.listed(workspace).await["state"], "ready");
        admitted_ready(&s.core.core(), workspace);

        let response = inspect_and_remove(&mut s, &id, Some(true)).await;
        assert_eq!(result(&response)["branch_kept"], workspace["vcs_ref"]);
        assert!(!folder(workspace).exists(), "it went with discard");
        assert_eq!(s.listed(workspace).await["state"], "put_away");
        assert!(has_branch(&s.repo, branch(workspace)));
    }
}

/// A workspace item whose token is not the one its inspect gives now is
/// refused with the item as it is now in `detail`, and nothing goes.
// verifies: FLOW-CLEAN-UP-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_item_whose_token_is_stale_is_refused_with_it_as_it_is_now() {
    let _turn = turn().await;
    let (mut s, clock) = listing().await;
    let (workspace, _) = archived_workspace(&mut s, &clock).await;
    let id = ws_id(&workspace);
    let seen = inspect(&mut s, &id).await;
    std::fs::write(folder(&workspace).join("late.txt"), "late\n").unwrap();

    // With `discard` too: the user approved losing what they were shown.
    let response = remove(&mut s, &id, &seen["token"], Some(true)).await;

    let error = error(&response);
    assert_eq!(kind(error), "invalid_state", "{error}");
    let now = detail(error);
    assert_eq!(now, inspect(&mut s, &id).await);
    assert_eq!(now["uncommitted"], 1, "{now}");
    assert_ne!(now["token"], seen["token"]);
    assert!(
        folder(&workspace).join("late.txt").exists(),
        "nothing removed"
    );
    assert_eq!(s.listed(&workspace).await["state"], "ready");
    admitted_ready(&s.core.core(), &workspace);
}

/// A workspace with a live session is refused with `session_busy`.
// verifies: FLOW-CLEAN-UP-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_with_a_live_session_is_refused_as_busy() {
    let _turn = turn().await;
    let (mut s, clock) = listing().await;
    let (workspace, session) = archived_workspace(&mut s, &clock).await;
    let id = ws_id(&workspace);
    let token = inspect(&mut s, &id).await["token"].clone();
    // Resumed meanwhile, which unarchives it and starts its backend.
    s.client
        .result("session.resume", json!({ "id": session }))
        .await;

    let response = remove(&mut s, &id, &token, Some(true)).await;

    let error = error(&response);
    assert_eq!(kind(error), "invalid_state", "{error}");
    assert_eq!(reason(error), Some("session_busy"), "{error}");
    assert!(folder(&workspace).exists());
    assert_eq!(s.listed(&workspace).await["state"], "ready");
}

/// A workspace item whose folder holds a repository of its own, a
/// submodule's or a nested one, is refused without `discard`; with it the
/// folder goes with every repository in it, and git's record of the
/// submodule's repository with the worktree's.
// verifies: FLOW-CLEAN-UP-R27
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_item_holding_a_repository_goes_only_with_discard() {
    let _turn = turn().await;
    let (mut s, clock) = listing().await;
    s.add_submodule();
    let (submodule, _) = archived_workspace(&mut s, &clock).await;
    check_out_submodules(&folder(&submodule));
    let (nested, _) = archived_workspace(&mut s, &clock).await;
    nested_repository(&folder(&nested).join("inner"));

    for workspace in [&submodule, &nested] {
        let id = ws_id(workspace);
        assert_eq!(inspect(&mut s, &id).await["holds_repository"], true);
        let refused = inspect_and_remove(&mut s, &id, None).await;
        refused_for_unsaved_work(&refused);
        assert!(folder(workspace).exists(), "nothing removed");
        assert_eq!(s.listed(workspace).await["state"], "ready");

        let response = inspect_and_remove(&mut s, &id, Some(true)).await;
        result(&response);
        assert!(!folder(workspace).exists(), "the folder went with discard");
        assert!(!listed_by_git(&s.repo, workspace));
        assert_eq!(s.listed(workspace).await["state"], "put_away");
    }
    let records = s.repo.join(".git").join("worktrees").join(id(&submodule));
    assert!(
        !records.exists(),
        "git's record, with the submodule's repository, is gone"
    );
}

/// The project's own folder is never an item: removing it is `not_found`,
/// and nothing of it changes.
// verifies: FLOW-CLEAN-UP-R14
#[tokio::test(flavor = "multi_thread")]
async fn the_projects_own_folder_is_never_removed() {
    let _turn = turn().await;
    let (mut s, _clock) = listing().await;
    let listed = s
        .client
        .result("workspace.list", json!({ "project_id": s.project["id"] }))
        .await;
    let root = listed[0].clone();
    assert_eq!(root["kind"], "root");
    std::fs::write(s.repo.join("mine.txt"), "mine\n").unwrap();

    let response = remove(&mut s, &ws_id(&root), &json!("any"), Some(true)).await;

    assert_eq!(kind(error(&response)), "not_found", "{response}");
    assert!(s.repo.join("mine.txt").exists());
    assert!(s.repo.join("README.md").exists());
}

// -- Leftover items (FLOW-CLEAN-UP-R13) -------------------------------------

/// A `leftover` item's folder, git's record, branch and workspace go.
// verifies: FLOW-CLEAN-UP-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_leftover_item_goes_with_its_branch() {
    let _turn = turn().await;
    let (mut s, clock) = listing().await;
    let workspace = leftover(&mut s, &clock).await;
    // Ignored files are no unsaved work.
    std::fs::write(folder(&workspace).join(".gitignore"), "*.log\n").unwrap();
    git(&folder(&workspace), &["add", ".gitignore"]);
    git(
        &folder(&workspace),
        &["commit", "--quiet", "-m", "ignore logs"],
    );
    git(
        &s.repo,
        &["merge", "--quiet", "--ff-only", branch(&workspace)],
    );
    std::fs::write(folder(&workspace).join("build.log"), "built\n").unwrap();

    let response = inspect_and_remove(&mut s, &ws_id(&workspace), None).await;

    let answer = result(&response);
    assert_eq!(answer["id"], ws_id(&workspace));
    assert!(answer.get("branch_kept").is_none(), "{answer}");
    assert!(!folder(&workspace).exists());
    assert!(!listed_by_git(&s.repo, &workspace));
    assert!(
        !has_branch(&s.repo, branch(&workspace)),
        "the branch is gone"
    );
    assert_eq!(
        s.listed(&workspace).await,
        Value::Null,
        "the workspace is gone"
    );
}

/// A leftover whose branch holds commits no other branch holds keeps the
/// branch without `discard`, and the answer names it; with `discard` the
/// branch goes too. One whose branch another worktree has checked out
/// keeps it either way.
// verifies: FLOW-CLEAN-UP-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_leftovers_branch_with_work_of_its_own_stays_unless_discarded() {
    let _turn = turn().await;
    let (mut s, clock) = listing().await;
    let kept = s.workspace().await;
    commit(&folder(&kept), "kept.txt");
    let discarded = s.workspace().await;
    commit(&folder(&discarded), "discarded.txt");
    let elsewhere = s.workspace().await;
    clock.time_passed(HOUR + Duration::from_secs(60));

    let response = inspect_and_remove(&mut s, &ws_id(&kept), None).await;
    assert_eq!(result(&response)["branch_kept"], kept["vcs_ref"]);
    assert!(!folder(&kept).exists());
    assert!(has_branch(&s.repo, branch(&kept)), "its commit is kept");
    assert_eq!(s.listed(&kept).await, Value::Null);

    let response = inspect_and_remove(&mut s, &ws_id(&discarded), Some(true)).await;
    assert!(result(&response).get("branch_kept").is_none(), "{response}");
    assert!(!has_branch(&s.repo, branch(&discarded)), "discarded");

    // Its folder moved off the branch, which a folder by hand checks out.
    git(&folder(&elsewhere), &["switch", "--quiet", "--detach"]);
    let by_hand = s.core.dir.path().join("by-hand");
    let path = by_hand.display().to_string();
    git(
        &s.repo,
        &["worktree", "add", "--quiet", &path, branch(&elsewhere)],
    );
    let response = inspect_and_remove(&mut s, &ws_id(&elsewhere), Some(true)).await;
    assert_eq!(result(&response)["branch_kept"], elsewhere["vcs_ref"]);
    assert!(has_branch(&s.repo, branch(&elsewhere)));
    assert!(by_hand.join("README.md").exists(), "the other folder stays");
}

/// A leftover with uncommitted files, or whose folder holds a repository
/// of its own, is refused without `discard`, and nothing goes; with it,
/// everything goes, the repository in the folder included.
// verifies: FLOW-CLEAN-UP-R13
// verifies: FLOW-CLEAN-UP-R27
#[tokio::test(flavor = "multi_thread")]
async fn a_leftover_holding_unsaved_work_or_a_repository_goes_only_with_discard() {
    let _turn = turn().await;
    let (mut s, clock) = listing().await;
    let dirty = s.workspace().await;
    std::fs::write(folder(&dirty).join("notes.txt"), "mine\n").unwrap();
    // A submodule checked out from its own repository holds nothing that
    // repository does not: only the repository of its own counts.
    s.add_submodule();
    let submodule = s.workspace().await;
    check_out_submodules(&folder(&submodule));
    clock.time_passed(HOUR + Duration::from_secs(60));
    let inspected = inspect(&mut s, &ws_id(&submodule)).await;
    assert_eq!(inspected["uncommitted"], 0, "{inspected}");
    assert_eq!(inspected["holds_repository"], true, "{inspected}");

    for workspace in [&dirty, &submodule] {
        let id = ws_id(workspace);
        assert_eq!(inspect(&mut s, &id).await["kind"], "leftover");
        let refused = inspect_and_remove(&mut s, &id, None).await;
        refused_for_unsaved_work(&refused);
        assert!(folder(workspace).exists(), "nothing removed");
        assert!(listed_by_git(&s.repo, workspace));
        assert_eq!(s.listed(workspace).await["state"], "ready");

        let response = inspect_and_remove(&mut s, &id, Some(true)).await;
        result(&response);
        assert!(!folder(workspace).exists(), "all of it went");
        assert!(!listed_by_git(&s.repo, workspace));
        assert_eq!(s.listed(workspace).await, Value::Null);
    }
}

/// A leftover that a session now uses is no longer one: its token is
/// refused with the item as it is now, a `workspace`, and nothing goes.
// verifies: FLOW-CLEAN-UP-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_leftover_a_session_now_uses_is_refused_as_changed() {
    let _turn = turn().await;
    let (mut s, clock) = listing().await;
    let workspace = leftover(&mut s, &clock).await;
    let id = ws_id(&workspace);
    let seen = inspect(&mut s, &id).await;
    assert_eq!(seen["kind"], "leftover");
    let session = session_in(&mut s, &workspace).await;

    let response = remove(&mut s, &id, &seen["token"], Some(true)).await;

    let error = error(&response);
    assert_eq!(kind(error), "invalid_state", "{error}");
    assert_eq!(detail(error)["kind"], "workspace", "{error}");
    assert!(folder(&workspace).exists());
    assert!(session_row(&mut s, &session).await.is_some());
}

// -- Branch items (FLOW-CLEAN-UP-R13, R14) ----------------------------------

/// A `branch` item is deleted; one with commits nothing else holds only
/// with `discard`, merged or not.
// verifies: FLOW-CLEAN-UP-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_branch_item_is_deleted_and_one_with_work_of_its_own_only_with_discard() {
    let _turn = turn().await;
    let (mut s, _clock) = listing().await;
    let project = s.project["id"].as_str().unwrap().to_string();
    git(&s.repo, &["branch", "ersa/stray"]);
    git(&s.repo, &["switch", "--quiet", "-c", "ersa/ahead"]);
    commit(&s.repo, "ahead.txt");
    git(&s.repo, &["switch", "--quiet", "main"]);

    let stray = format!("b:{project}:ersa/stray");
    let response = inspect_and_remove(&mut s, &stray, None).await;
    assert_eq!(result(&response)["id"], stray.as_str());
    assert!(result(&response).get("branch_kept").is_none());
    assert!(!has_branch(&s.repo, "ersa/stray"));

    let ahead = format!("b:{project}:ersa/ahead");
    assert_eq!(inspect(&mut s, &ahead).await["unshared"], 1);
    refused_for_unsaved_work(&inspect_and_remove(&mut s, &ahead, None).await);
    assert!(has_branch(&s.repo, "ersa/ahead"));
    // Squashed into main: merged, but its commit still exists nowhere else.
    git(&s.repo, &["merge", "--quiet", "--squash", "ersa/ahead"]);
    git(&s.repo, &["commit", "--quiet", "-m", "squashed"]);
    let inspected = inspect(&mut s, &ahead).await;
    assert_eq!(inspected["merged"], true, "{inspected}");
    refused_for_unsaved_work(&inspect_and_remove(&mut s, &ahead, None).await);
    assert!(has_branch(&s.repo, "ersa/ahead"));

    let response = inspect_and_remove(&mut s, &ahead, Some(true)).await;
    result(&response);
    assert!(!has_branch(&s.repo, "ersa/ahead"));
}

/// A branch item whose token is stale is refused with it as it is now, and
/// stays.
// verifies: FLOW-CLEAN-UP-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_branch_item_whose_token_is_stale_is_refused() {
    let _turn = turn().await;
    let (mut s, _clock) = listing().await;
    let project = s.project["id"].as_str().unwrap().to_string();
    git(&s.repo, &["branch", "ersa/stray"]);
    let id = format!("b:{project}:ersa/stray");
    let seen = inspect(&mut s, &id).await;
    git(&s.repo, &["switch", "--quiet", "ersa/stray"]);
    commit(&s.repo, "later.txt");
    git(&s.repo, &["switch", "--quiet", "main"]);

    let response = remove(&mut s, &id, &seen["token"], Some(true)).await;

    let error = error(&response);
    assert_eq!(kind(error), "invalid_state", "{error}");
    let now = detail(error);
    assert_eq!(now["unshared"], 1, "{now}");
    assert_eq!(now, inspect(&mut s, &id).await);
    assert!(has_branch(&s.repo, "ersa/stray"));
}

/// Nothing deletes a branch not named `ersa/…` that no workspace names, nor
/// one a workspace names or a worktree has checked out: none is an item.
// verifies: FLOW-CLEAN-UP-R14
#[tokio::test(flavor = "multi_thread")]
async fn no_branch_but_an_unused_ersa_branch_is_deleted() {
    let _turn = turn().await;
    let (mut s, _clock) = listing().await;
    let project = s.project["id"].as_str().unwrap().to_string();
    git(&s.repo, &["branch", "feature/mine"]);
    let workspace = s.workspace().await;

    for name in ["feature/mine", branch(&workspace), "main"] {
        let id = format!("b:{project}:{name}");
        let response = remove(&mut s, &id, &json!("any"), Some(true)).await;
        assert_eq!(kind(error(&response)), "not_found", "{name}: {response}");
        assert!(has_branch(&s.repo, name), "{name} stays");
    }
}

// -- Folder items (FLOW-CLEAN-UP-R13, R14) ----------------------------------

/// A `folder` item is deleted only with `discard`.
// verifies: FLOW-CLEAN-UP-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_item_is_deleted_only_with_discard() {
    let _turn = turn().await;
    let (mut s, _clock) = listing().await;
    let project = s.project["id"].as_str().unwrap().to_string();
    let stray = worktrees(&s).join("stray");
    std::fs::create_dir_all(stray.join("deep")).unwrap();
    std::fs::write(stray.join("deep").join("a.bin"), vec![1u8; 300]).unwrap();
    let id = format!("f:{project}:stray");

    for discard in [None, Some(false)] {
        refused_for_unsaved_work(&inspect_and_remove(&mut s, &id, discard).await);
        assert!(stray.join("deep").join("a.bin").exists(), "nothing removed");
    }

    let response = inspect_and_remove(&mut s, &id, Some(true)).await;
    assert_eq!(result(&response)["id"], id.as_str());
    assert!(result(&response).get("branch_kept").is_none());
    assert!(!stray.exists(), "the folder is gone");
    assert!(worktrees(&s).exists(), "the project's folder stays");
}

/// A folder item whose token is stale is refused with it as it is now.
// verifies: FLOW-CLEAN-UP-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_item_whose_token_is_stale_is_refused() {
    let _turn = turn().await;
    let (mut s, _clock) = listing().await;
    let project = s.project["id"].as_str().unwrap().to_string();
    let stray = worktrees(&s).join("stray");
    std::fs::create_dir_all(&stray).unwrap();
    std::fs::write(stray.join("a.bin"), vec![1u8; 300]).unwrap();
    let id = format!("f:{project}:stray");
    let seen = inspect(&mut s, &id).await;
    std::fs::write(stray.join("b.bin"), vec![2u8; 10]).unwrap();

    let response = remove(&mut s, &id, &seen["token"], Some(true)).await;

    let error = error(&response);
    assert_eq!(kind(error), "invalid_state", "{error}");
    assert_eq!(detail(error)["size_bytes"], 310, "{error}");
    assert!(stray.join("a.bin").exists());
}

/// A folder Ersa could not measure in time has a token that is new on
/// every inspect, so its removal is refused whatever token is sent,
/// `discard` or not, and nothing of it is removed (issue #543, planning
/// pass of 2026-11-26).
// verifies: FLOW-CLEAN-UP-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_not_measured_in_time_is_never_removed() {
    let _turn = turn().await;
    let (mut s, _clock) = listing_with(|config| config.measure_limit = Duration::ZERO).await;
    let project = s.project["id"].as_str().unwrap().to_string();
    let stray = worktrees(&s).join("stray");
    std::fs::create_dir_all(&stray).unwrap();
    std::fs::write(stray.join("a.bin"), vec![1u8; 300]).unwrap();
    let id = format!("f:{project}:stray");

    for discard in [Some(true), None] {
        let seen = inspect(&mut s, &id).await;
        assert!(seen.get("size_bytes").is_none(), "{seen}");
        let response = remove(&mut s, &id, &seen["token"], discard).await;
        let error = error(&response);
        assert_eq!(kind(error), "invalid_state", "{error}");
        assert!(detail(error).get("size_bytes").is_none(), "{error}");
        assert!(stray.join("a.bin").exists(), "nothing removed");
    }
}

/// Nothing deletes a folder outside the worktrees root, nor a workspace's
/// folder as a folder: neither is a folder item.
// verifies: FLOW-CLEAN-UP-R14
#[tokio::test(flavor = "multi_thread")]
async fn no_folder_outside_the_worktrees_root_is_deleted() {
    let _turn = turn().await;
    let (mut s, _clock) = listing().await;
    let project = s.project["id"].as_str().unwrap().to_string();
    let workspace = s.workspace().await;
    let outside = s.core.dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();

    for name in ["../repo", "..", "../../outside", id(&workspace)] {
        let id = format!("f:{project}:{name}");
        let response = remove(&mut s, &id, &json!("any"), Some(true)).await;
        assert_eq!(kind(error(&response)), "not_found", "{name}: {response}");
    }
    assert!(s.repo.join("README.md").exists());
    assert!(outside.exists());
    assert!(folder(&workspace).join("README.md").exists());
}

/// A link at the project's folder under the worktrees root (a junction on
/// Windows) is not followed: the folders where it points are outside the
/// root, so none is listed or deleted.
// verifies: FLOW-CLEAN-UP-R14
#[tokio::test(flavor = "multi_thread")]
async fn no_folder_is_deleted_through_a_link_at_the_projects_folder() {
    let _turn = turn().await;
    let (mut s, _clock) = listing().await;
    let project = s.project["id"].as_str().unwrap().to_string();
    let outside = s.core.dir.path().join("outside");
    std::fs::create_dir_all(outside.join("victim")).unwrap();
    std::fs::write(
        outside.join("victim").join("mine.txt"),
        "mine
",
    )
    .unwrap();
    let link = worktrees(&s);
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    link_folder(&outside, &link);
    assert!(
        link.join("victim").join("mine.txt").exists(),
        "the link leads there"
    );

    let listed = list(&mut s, 30).await;
    assert_eq!(listed["items"], json!([]), "{listed}");
    let id = format!("f:{project}:victim");
    let response = remove(&mut s, &id, &json!("any"), Some(true)).await;

    assert_eq!(kind(error(&response)), "not_found", "{response}");
    assert!(outside.join("victim").join("mine.txt").exists());
}

/// Makes `link` a link to the folder `target`: a junction on Windows,
/// which needs no privilege, a symbolic link elsewhere.
fn link_folder(target: &Path, link: &Path) {
    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .expect("cmd runs");
        assert!(status.success(), "mklink /J failed");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("symlink");
}

// -- Refusals for any kind --------------------------------------------------

/// An id that names nothing is `not_found`.
// verifies: FLOW-CLEAN-UP-R13
#[tokio::test(flavor = "multi_thread")]
async fn removing_what_is_no_item_is_not_found() {
    let _turn = turn().await;
    let (mut s, _clock) = listing().await;
    for id in ["nonsense", "w:w-none", "b:p-none:ersa/x", "f:p-none:x"] {
        let response = remove(&mut s, id, &json!("any"), Some(true)).await;
        assert_eq!(kind(error(&response)), "not_found", "{id}: {response}");
    }
}

// -- Waiting its turn, and the folder's tries (PROTO-CORE-API-R122, R124, R131)

/// A removal whose git drops its record of the worktree and then fails
/// goes on to delete the folder, as every removal does.
// verifies: PROTO-CORE-API-R122
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_goes_on_to_the_folder_when_git_fails_after_dropping_its_record() {
    let _turn = turn().await;
    let git_drops = Arc::new(support::deleting::FailingGit::new(
        support::deleting::Removal::DropsRecordThenFails,
        None,
    ));
    let (mut s, clock) = listing_with(|config| {
        config.command_transport = Some(ersa_core::CommandTransport(git_drops));
    })
    .await;
    let (workspace, _) = archived_workspace(&mut s, &clock).await;
    let gone = leftover(&mut s, &clock).await;

    for workspace in [&workspace, &gone] {
        let response = inspect_and_remove(&mut s, &ws_id(workspace), None).await;
        result(&response);
        assert!(!folder(workspace).exists(), "the folder is gone");
        assert!(!listed_by_git(&s.repo, workspace));
    }
    assert_eq!(s.listed(&workspace).await["state"], "put_away");
    assert_eq!(s.listed(&gone).await, Value::Null);
}

/// A removal waits its turn at the project's worktrees; past the wait it
/// answers `vcs_error` and removes nothing.
// verifies: PROTO-CORE-API-R124
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_that_waited_too_long_for_its_turn_removes_nothing() {
    let _turn = turn().await;
    let (mut s, clock) = listing_with(|config| {
        config.worktree_turn_wait = Duration::from_millis(300);
    })
    .await;
    let project = s.project["id"].as_str().unwrap().to_string();
    let (workspace, _) = archived_workspace(&mut s, &clock).await;
    let gone = leftover(&mut s, &clock).await;
    git(&s.repo, &["branch", "ersa/stray"]);
    let stray = format!("b:{project}:ersa/stray");
    let mut tokens = Vec::new();
    for id in [ws_id(&workspace), ws_id(&gone), stray.clone()] {
        tokens.push((id.clone(), inspect(&mut s, &id).await["token"].clone()));
    }

    // Core's own turn, held as a request that changes the worktrees holds it.
    let core = s.core.core();
    let turn = ersa_core::workspace::project_turn(
        &core.admission,
        s.project["id"].as_str().unwrap(),
        Duration::from_secs(5),
    )
    .await
    .expect("the turn");
    for (id, token) in &tokens {
        let response = remove(&mut s, id, token, Some(true)).await;
        assert_eq!(kind(error(&response)), "vcs_error", "{id}: {response}");
    }
    drop(turn);

    assert!(folder(&workspace).exists());
    assert_eq!(s.listed(&workspace).await["state"], "ready");
    admitted_ready(&core, &workspace);
    assert!(folder(&gone).exists());
    assert_eq!(s.listed(&gone).await["state"], "ready");
    assert!(has_branch(&s.repo, "ersa/stray"));
}

/// While a try at a folder that `cleanup.remove` gave up on has not ended,
/// no other removal of that folder or of the workspace begins: another
/// `cleanup.remove` and a `workspace.remove` answer `invalid_state`, and
/// the sweep leaves it. Once the try ends, it can be removed.
// verifies: PROTO-CORE-API-R131
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_holds_the_workspace_while_a_try_it_gave_up_on_is_stuck() {
    let _turn = turn().await;
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let clock = Arc::new(ersa_core::SweepClock::default());
    let sweeps = ersa_core::Sweeps::Driven(Arc::clone(&clock));
    let mut s = stuck_folder_setup_with(&gate, ersa_core::workspace::WORKTREE_TURN_WAIT, |c| {
        c.sweeps = sweeps;
    })
    .await;
    passes(&clock, 1).await;
    let workspace = leftover(&mut s, &clock).await;
    let item = ws_id(&workspace);
    let token = inspect(&mut s, &item).await["token"].clone();

    let removing = s
        .client
        .request("cleanup.remove", json!({ "id": item, "token": token }))
        .await;
    reached(&gate).await;
    let answer = s.client.response(removing).await;
    assert_eq!(
        kind(error(&answer)),
        "vcs_error",
        "the try ran out: {answer}"
    );

    for _ in 0..2 {
        let token = json!("any");
        let response = remove(&mut s, &item, &token, Some(true)).await;
        assert_eq!(kind(error(&response)), "invalid_state", "{response}");
        let again = json!({ "id": workspace["id"], "force": true });
        let error = s.client.error("workspace.remove", again).await;
        assert_eq!(kind(&error), "invalid_state", "{error}");
    }
    let before = clock.ended();
    clock.sweep_again();
    passes(&clock, before + 1).await;
    assert!(folder(&workspace).exists(), "nothing else removed it");

    // The stuck try returns; then the hold goes.
    gate.go();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let phase = s.core.core().admission.snapshot();
        let phase = phase.workspace(id(&workspace)).map(|w| w.phase);
        if phase == Some(ersa_core::admission::WorkspacePhase::Error) {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "held: {phase:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
