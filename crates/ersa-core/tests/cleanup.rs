//! The Clean up list (FLOW-CLEAN-UP, The list): `cleanup.list` and
//! `cleanup.inspect` over the socket, against real git repositories and the
//! fake backend. The sweep's clock is the test's (`Sweeps::Driven`), so a
//! test moves time on without a pass: the list reads the same clock.

mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ersa_core::{Adapter, CoreConfig, SweepClock, Sweeps};
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter};
use support::put_away::*;
use support::{assert_valid, git, kind, start_with_adapters, tempdir};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const HOUR: Duration = Duration::from_secs(60 * 60);

/// A Core with the fake backend and a git project, whose sweep runs only
/// at start and after an archive, by the returned clock.
async fn listing_with(more: impl FnOnce(&mut CoreConfig)) -> (Setup, Arc<SweepClock>) {
    listing_over(FakeAdapter::new(1), more).await
}

async fn listing() -> (Setup, Arc<SweepClock>) {
    listing_with(|_| {}).await
}

async fn passes(clock: &SweepClock, count: u64) {
    tokio::time::timeout(Duration::from_secs(120), clock.until_ended(count))
        .await
        .expect("the sweep ended in time");
}

/// `cleanup.list` with `inactive_days`, checked against the schema.
async fn list(s: &mut Setup, days: u32) -> Value {
    let response = s
        .client
        .call("cleanup.list", json!({ "inactive_days": days }))
        .await;
    assert_valid("cleanup.list", &response);
    response["result"].clone()
}

/// The ids of the items of `kind` in `listed`.
fn ids(listed: &Value, kind: &str) -> Vec<String> {
    let mut ids: Vec<String> = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == kind)
        .map(|item| item["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

fn item<'a>(listed: &'a Value, id: &str) -> Option<&'a Value> {
    listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == id)
}

/// `cleanup.inspect` of `id`, checked against the schema.
async fn inspect(s: &mut Setup, id: &str) -> Value {
    let response = s.client.call("cleanup.inspect", json!({ "id": id })).await;
    assert_valid("cleanup.inspect", &response);
    assert!(response.get("result").is_some(), "{response}");
    response["result"].clone()
}

fn ws_id(workspace: &Value) -> String {
    format!("w:{}", id(workspace))
}

/// A session of the fake backend made in `workspace`, with a turn behind
/// it, live and idle.
async fn live_in(s: &mut Setup, workspace: &Value) -> String {
    let params = json!({ "workspace_id": workspace["id"], "backend": fake::BACKEND });
    let session = s.client.result("session.create", params).await;
    let session = session["id"].as_str().unwrap().to_string();
    let mut watcher = s.core.client().await;
    let subscribe = json!({ "id": session, "from_seq": 0 });
    watcher.result("session.subscribe", subscribe).await;
    watcher.events_until("session.started").await;
    // The fake backend finishes a turn whose message is `idle`.
    let send = json!({ "id": session, "text": "idle" });
    s.client.result("session.send", send).await;
    support::wait_for_state(&mut s.client, &session, "idle").await;
    session
}

/// A session of the fake backend in `workspace`, with a turn behind it,
/// then stopped.
async fn session_in(s: &mut Setup, workspace: &Value) -> String {
    let session = s.stopped_session(workspace).await;
    // Subscribed to, it would not count as unused.
    s.client
        .result("session.unsubscribe", json!({ "id": session }))
        .await;
    session
}

/// Archives `session`, then waits for the pass the archive asks for.
async fn archive(s: &mut Setup, clock: &SweepClock, session: &str) {
    let before = clock.ended();
    let params = json!({ "id": session, "archived": true });
    s.client.result("session.set_archived", params).await;
    passes(clock, before + 1).await;
}

fn worktrees(s: &Setup) -> std::path::PathBuf {
    let root = s.core.dir.path().join("worktrees");
    let name = s.project["name"].as_str().unwrap();
    root.join(name)
}

// -- Workspaces (FLOW-CLEAN-UP-R11) ---------------------------------------

/// A workspace whose sessions are all archived, kept by the sweep for its
/// uncommitted file, is listed with its sessions, project, folder and
/// branch; the project's own folder never is.
// verifies: FLOW-CLEAN-UP-R11
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_whose_sessions_are_all_archived_is_listed() {
    let (mut s, clock) = listing().await;
    let workspace = s.workspace().await;
    let first = session_in(&mut s, &workspace).await;
    let second = session_in(&mut s, &workspace).await;
    std::fs::write(folder(&workspace).join("notes.txt"), "mine\n").unwrap();
    archive(&mut s, &clock, &first).await;

    // One session not archived, and active: not listed.
    assert!(ids(&list(&mut s, 30).await, "workspace").is_empty());

    archive(&mut s, &clock, &second).await;
    let listed = list(&mut s, 90).await;
    assert_eq!(ids(&listed, "workspace"), vec![ws_id(&workspace)]);
    let it = item(&listed, &ws_id(&workspace)).unwrap();
    assert_eq!(it["project_id"], s.project["id"]);
    assert_eq!(it["workspace_id"], workspace["id"]);
    assert_eq!(it["path"], workspace["path"]);
    assert_eq!(it["branch"], workspace["vcs_ref"]);
    let mut sessions = vec![first, second];
    sessions.sort();
    assert_eq!(it["session_ids"], json!(sessions));
    // The latest of its sessions' last activity.
    let sessions_listed = s.client.result("session.list", json!({})).await;
    let latest = sessions_listed
        .as_array()
        .unwrap()
        .iter()
        .filter(|x| sessions.iter().any(|id| x["id"] == id.as_str()))
        .filter_map(|x| x["last_activity_at"].as_str())
        .max();
    assert_eq!(it["last_activity_at"].as_str(), latest, "{it}");
    assert_eq!(listed["unread"], json!([]));
    // Only isolated workspaces: the root workspace is no item.
    assert_eq!(listed["items"].as_array().unwrap().len(), 1, "{listed}");
}

/// Sessions inactive for `inactive_days` list their workspace, and only
/// past that many days; one pinned, unseen, subscribed to or live keeps it
/// off the list.
// verifies: FLOW-CLEAN-UP-R11
#[tokio::test(flavor = "multi_thread")]
async fn sessions_inactive_that_many_days_list_their_workspace() {
    let (mut s, clock) = listing().await;
    let idle = s.workspace().await;
    session_in(&mut s, &idle).await;
    let pinned = s.workspace().await;
    let pin = session_in(&mut s, &pinned).await;
    let arrange = json!({ "id": pin, "pinned": true });
    s.client.result("session.arrange", arrange).await;
    let unseen = s.workspace().await;
    let mark = session_in(&mut s, &unseen).await;
    let arrange = json!({ "id": mark, "unseen": true });
    s.client.result("session.arrange", arrange).await;
    let watched = s.workspace().await;
    let watch = session_in(&mut s, &watched).await;
    let mut watcher = s.core.client().await;
    let subscribe = json!({ "id": watch, "from_seq": 0 });
    watcher.result("session.subscribe", subscribe).await;
    let running = s.workspace().await;
    live_in(&mut s, &running).await;

    assert!(ids(&list(&mut s, 30).await, "workspace").is_empty());

    clock.time_passed(31 * DAY);
    assert_eq!(
        ids(&list(&mut s, 30).await, "workspace"),
        vec![ws_id(&idle)]
    );
    assert!(ids(&list(&mut s, 60).await, "workspace").is_empty());
    clock.time_passed(30 * DAY);
    assert_eq!(
        ids(&list(&mut s, 60).await, "workspace"),
        vec![ws_id(&idle)]
    );
    assert!(ids(&list(&mut s, 90).await, "workspace").is_empty());
}

/// `inactive_days` is 30, 60 or 90; anything else is `invalid_params`.
// verifies: FLOW-CLEAN-UP-R11
#[tokio::test(flavor = "multi_thread")]
async fn inactive_days_other_than_thirty_sixty_or_ninety_is_refused() {
    let (mut s, _clock) = listing().await;
    for days in [30, 60, 90] {
        list(&mut s, days).await;
    }
    for days in [0, 1, 45, 365] {
        let params = json!({ "inactive_days": days });
        // JSON-RPC's own code for invalid params.
        let response = s.client.call("cleanup.list", params).await;
        assert_eq!(response["error"]["code"], -32602, "{days}: {response}");
    }
}

// -- Leftovers (FLOW-CLEAN-UP-R11, R23) -----------------------------------

/// A workspace no session refers to is a leftover once it was made an
/// hour ago. One whose sessions are removed turns from a `workspace` item
/// into a `leftover` and keeps its id; a put-away one is a leftover only
/// when nothing of its folder is left.
// verifies: FLOW-CLEAN-UP-R11
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_no_session_refers_to_is_a_leftover_after_an_hour() {
    let (mut s, clock) = listing().await;
    let bare = s.workspace().await;
    let used = s.workspace().await;
    let session = session_in(&mut s, &used).await;
    std::fs::write(folder(&used).join("notes.txt"), "mine\n").unwrap();
    archive(&mut s, &clock, &session).await;
    let away = s.workspace().await;
    s.put_away(&away).await.expect("put away");
    let half = s.workspace().await;
    s.put_away(&half).await.expect("put away");
    // What a put-away that failed part way left.
    std::fs::create_dir_all(folder(&half)).unwrap();
    std::fs::write(folder(&half).join("left.txt"), "left\n").unwrap();

    let listed = list(&mut s, 30).await;
    assert!(ids(&listed, "leftover").is_empty(), "{listed}");
    assert_eq!(ids(&listed, "workspace"), vec![ws_id(&used)]);

    s.client
        .result("session.remove", json!({ "id": session }))
        .await;
    clock.time_passed(HOUR + Duration::from_secs(60));
    let listed = list(&mut s, 30).await;
    let mut leftovers = vec![ws_id(&bare), ws_id(&used), ws_id(&away)];
    leftovers.sort();
    assert_eq!(ids(&listed, "leftover"), leftovers);
    assert!(ids(&listed, "workspace").is_empty());
    let it = item(&listed, &ws_id(&away)).unwrap();
    assert_eq!(it["workspace_id"], away["id"]);
    assert_eq!(it["path"], away["path"]);
    assert_eq!(it["branch"], away["vcs_ref"]);
    // An id is the same for the same thing from one list to the next.
    assert_eq!(ids(&list(&mut s, 30).await, "leftover"), leftovers);
}

// -- Branches (FLOW-CLEAN-UP-R11) -----------------------------------------

/// A local `ersa/…` branch is listed when no workspace names it and no
/// worktree has it checked out; another name never is, nor the branch a
/// put-away workspace had checked out.
// verifies: FLOW-CLEAN-UP-R11
#[tokio::test(flavor = "multi_thread")]
async fn an_ersa_branch_nothing_uses_is_listed() {
    let (mut s, _clock) = listing().await;
    let project = s.project["id"].as_str().unwrap().to_string();
    git(&s.repo, &["branch", "ersa/stray"]);
    git(&s.repo, &["branch", "feature/mine"]);
    git(&s.repo, &["branch", "ersa/elsewhere"]);
    let elsewhere = s.core.dir.path().join("by-hand");
    let outside = elsewhere.display().to_string();
    git(
        &s.repo,
        &["worktree", "add", "--quiet", &outside, "ersa/elsewhere"],
    );
    let workspace = s.workspace().await;
    // Its folder moved to a branch of its own, then was put away.
    let away = s.workspace().await;
    git(
        &folder(&away),
        &["switch", "--quiet", "-c", "ersa/switched"],
    );
    s.put_away(&away).await.expect("put away");

    let listed = list(&mut s, 30).await;
    assert_eq!(
        ids(&listed, "branch"),
        vec![format!("b:{project}:ersa/stray")]
    );
    let it = item(&listed, &format!("b:{project}:ersa/stray")).unwrap();
    assert_eq!(it["branch"], "ersa/stray");
    assert_eq!(it["project_id"], project.as_str());
    assert!(it.get("path").is_none());
    let _ = workspace;
}

// -- Folders (FLOW-CLEAN-UP-R11) ------------------------------------------

/// A folder in the project's folder under the worktrees root is listed
/// when no workspace and no git worktree record names it; a file there, a
/// workspace's folder, a worktree made there by hand and a folder outside
/// the root are not.
// verifies: FLOW-CLEAN-UP-R11
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_nothing_names_under_the_worktrees_root_is_listed() {
    let (mut s, _clock) = listing().await;
    let project = s.project["id"].as_str().unwrap().to_string();
    s.workspace().await;
    let here = worktrees(&s);
    std::fs::create_dir_all(here.join("stray").join("deep")).unwrap();
    std::fs::write(here.join("stray.txt"), "a file\n").unwrap();
    let by_hand = here.join("by-hand").display().to_string();
    git(
        &s.repo,
        &["worktree", "add", "--quiet", "-b", "mine", &by_hand],
    );
    std::fs::create_dir_all(s.core.dir.path().join("outside")).unwrap();
    // What a put-away that failed part way left: git has no record of it,
    // and the workspace still names it.
    let away = s.workspace().await;
    s.put_away(&away).await.expect("put away");
    std::fs::create_dir_all(folder(&away)).unwrap();
    std::fs::write(folder(&away).join("left.txt"), "left\n").unwrap();

    let listed = list(&mut s, 30).await;
    let stray = format!("f:{project}:stray");
    assert_eq!(ids(&listed, "folder"), vec![stray.clone()]);
    let it = item(&listed, &stray).unwrap();
    let path = Path::new(it["path"].as_str().unwrap());
    assert!(path.ends_with(Path::new("stray")), "{it}");
    assert!(path.join("deep").is_dir());
    assert!(it.get("branch").is_none());
}

/// A project whose git calls fail contributes no branch or folder item and
/// is named in `unread` with the reason; the others are listed as before.
// verifies: FLOW-CLEAN-UP-R11
#[tokio::test(flavor = "multi_thread")]
async fn a_project_git_cannot_read_is_named_in_unread() {
    let (mut s, _clock) = listing().await;
    git(&s.repo, &["branch", "ersa/stray"]);
    let other = s.core.dir.path().join("other");
    support::git_repo(&other);
    let add = json!({ "host_id": "local", "path": other.display().to_string(), "trusted": false });
    let broken = s.client.result("project.add", add).await;
    let root = s.core.dir.path().join("worktrees");
    let stray = root.join(broken["name"].as_str().unwrap()).join("stray");
    std::fs::create_dir_all(&stray).unwrap();
    // No longer a repository.
    std::fs::remove_dir_all(other.join(".git")).unwrap();

    let listed = list(&mut s, 30).await;
    let unread = listed["unread"].as_array().unwrap();
    assert_eq!(unread.len(), 1, "{listed}");
    assert_eq!(unread[0]["project_id"], broken["id"]);
    assert!(!unread[0]["message"].as_str().unwrap().is_empty());
    let named_broken = |item: &Value| item["project_id"] == broken["id"];
    assert!(!listed["items"].as_array().unwrap().iter().any(named_broken));
    assert_eq!(ids(&listed, "branch").len(), 1, "{listed}");
}

// -- Inspect (FLOW-CLEAN-UP-R12, R26) -------------------------------------

/// A workspace item's counts are `workspace.changes`'s; its size is what
/// its folder's files take; its token stays while nothing changes and
/// changes with what removing it would lose.
// verifies: FLOW-CLEAN-UP-R12
#[tokio::test(flavor = "multi_thread")]
async fn inspecting_a_workspace_counts_what_workspace_changes_counts() {
    let (mut s, clock) = listing().await;
    let workspace = s.workspace().await;
    let session = session_in(&mut s, &workspace).await;
    let dir = folder(&workspace);
    std::fs::write(dir.join("notes.txt"), "mine\n").unwrap();
    std::fs::write(dir.join(".gitignore"), "build/\n").unwrap();
    std::fs::create_dir_all(dir.join("build")).unwrap();
    std::fs::write(dir.join("build").join("out.bin"), vec![0u8; 5000]).unwrap();
    commit(&dir, "committed.txt");
    archive(&mut s, &clock, &session).await;

    let id = ws_id(&workspace);
    let first = inspect(&mut s, &id).await;
    let changes = s
        .client
        .result("workspace.changes", json!({ "id": workspace["id"] }))
        .await;
    for field in [
        "uncommitted",
        "ignored",
        "unshared",
        "stranded",
        "unrecorded",
    ] {
        assert_eq!(first[field], changes[field], "{field}: {first} {changes}");
    }
    assert_eq!(first["uncommitted"], 2);
    assert_eq!(first["ignored"], 1);
    assert_eq!(first["unshared"], 1);
    assert_eq!(first["holds_repository"], false);
    assert_eq!(first["kind"], "workspace");
    assert_eq!(first["session_ids"], json!([session]));
    let size = first["size_bytes"].as_u64().expect("measured");
    assert!(size >= 5000, "{first}");
    assert_eq!(inspect(&mut s, &id).await["token"], first["token"]);

    std::fs::write(dir.join("notes.txt"), "mine, changed\n").unwrap();
    let changed = inspect(&mut s, &id).await;
    assert_ne!(changed["token"], first["token"]);
    assert_eq!(changed["uncommitted"], 2);
}

/// `holds_repository` is true for a folder with a submodule checked out,
/// for one whose submodule's repository git still keeps after its files
/// are gone, and for a nested repository the ignore rules do not ignore;
/// false for an ignored one and for none. The token changes with it.
// verifies: FLOW-CLEAN-UP-R26
#[tokio::test(flavor = "multi_thread")]
async fn inspect_says_when_a_folder_holds_a_repository_of_its_own() {
    let (mut s, clock) = listing().await;
    s.add_submodule();
    let workspace = s.workspace().await;
    let session = session_in(&mut s, &workspace).await;
    let dir = folder(&workspace);
    // Kept by the sweep for this, so it stays a `workspace` item.
    std::fs::write(dir.join("notes.txt"), "mine\n").unwrap();
    archive(&mut s, &clock, &session).await;
    let id = ws_id(&workspace);
    let plain = inspect(&mut s, &id).await;
    assert_eq!(plain["holds_repository"], false, "{plain}");

    // A submodule's repository kept with git's record of the folder, and
    // nothing else changed: the token changes with `holds_repository`
    // alone.
    let record = git(&dir, &["rev-parse", "--absolute-git-dir"]);
    let modules = Path::new(record.trim()).join("modules");
    std::fs::create_dir_all(&modules).unwrap();
    let kept_only = inspect(&mut s, &id).await;
    assert_eq!(kept_only["holds_repository"], true, "{kept_only}");
    assert_eq!(kept_only["uncommitted"], plain["uncommitted"]);
    assert_ne!(kept_only["token"], plain["token"]);
    std::fs::remove_dir_all(&modules).unwrap();
    assert_eq!(inspect(&mut s, &id).await["token"], plain["token"]);

    // An ignored nested repository: still none of its own that counts.
    std::fs::write(dir.join(".gitignore"), "vendor/\n").unwrap();
    nested_repository(&dir.join("vendor"));
    let ignored = inspect(&mut s, &id).await;
    assert_eq!(ignored["holds_repository"], false, "{ignored}");

    // A nested repository that is not ignored.
    nested_repository(&dir.join("lib"));
    let nested = inspect(&mut s, &id).await;
    assert_eq!(nested["holds_repository"], true, "{nested}");
    assert_ne!(nested["token"], ignored["token"]);
    std::fs::remove_dir_all(dir.join("lib")).unwrap();
    assert_eq!(inspect(&mut s, &id).await["holds_repository"], false);

    check_out_submodules(&dir);
    let checked_out = inspect(&mut s, &id).await;
    assert_eq!(checked_out["holds_repository"], true, "{checked_out}");
    // Its files gone, git still keeps its repository with its record of
    // the folder.
    git(
        &dir,
        &["submodule", "deinit", "--quiet", "--force", "--all"],
    );
    let kept = inspect(&mut s, &id).await;
    assert_eq!(kept["holds_repository"], true, "{kept}");
}

/// A branch item's `unshared` counts the commits nothing else holds, and
/// `merged` is true once the base branch holds what it changes, merged or
/// squashed in; the token changes with either.
// verifies: FLOW-CLEAN-UP-R12
#[tokio::test(flavor = "multi_thread")]
async fn inspecting_a_branch_says_what_only_it_holds_and_whether_it_is_merged() {
    let (mut s, _clock) = listing().await;
    let project = s.project["id"].as_str().unwrap().to_string();
    let repo = s.repo.clone();
    let main = git(&repo, &["branch", "--show-current"]).trim().to_string();
    git(&repo, &["switch", "--quiet", "-c", "ersa/squashed"]);
    commit(&repo, "one.txt");
    commit(&repo, "two.txt");
    git(&repo, &["switch", "--quiet", "-c", "ersa/merged"]);
    git(&repo, &["switch", "--quiet", &main]);

    let squashed = format!("b:{project}:ersa/squashed");
    let before = inspect(&mut s, &squashed).await;
    assert_eq!(
        before["unshared"], 0,
        "ersa/merged holds them too: {before}"
    );
    assert_eq!(before["merged"], false);
    assert!(before.get("path").is_none());

    git(&repo, &["branch", "--quiet", "-D", "ersa/merged"]);
    let alone = inspect(&mut s, &squashed).await;
    assert_eq!(alone["unshared"], 2, "{alone}");
    assert_eq!(alone["merged"], false);
    assert_ne!(alone["token"], before["token"]);

    git(&repo, &["merge", "--quiet", "--squash", "ersa/squashed"]);
    git(&repo, &["commit", "--quiet", "-m", "squashed"]);
    let squashed_in = inspect(&mut s, &squashed).await;
    assert_eq!(squashed_in["merged"], true, "{squashed_in}");
    assert_eq!(squashed_in["unshared"], 2);
    assert_ne!(squashed_in["token"], alone["token"]);

    git(&repo, &["branch", "ersa/behind", &format!("{main}~1")]);
    let behind = inspect(&mut s, &format!("b:{project}:ersa/behind")).await;
    assert_eq!(behind["merged"], true, "{behind}");
    assert_eq!(behind["unshared"], 0);
}

/// A folder item's size is what its files take, and its token changes
/// when what is in it does.
// verifies: FLOW-CLEAN-UP-R12
#[tokio::test(flavor = "multi_thread")]
async fn inspecting_a_folder_measures_it() {
    let (mut s, _clock) = listing().await;
    let project = s.project["id"].as_str().unwrap().to_string();
    let stray = worktrees(&s).join("stray");
    std::fs::create_dir_all(stray.join("deep")).unwrap();
    std::fs::write(stray.join("a.bin"), vec![1u8; 300]).unwrap();
    std::fs::write(stray.join("deep").join("b.bin"), vec![2u8; 200]).unwrap();

    let id = format!("f:{project}:stray");
    let first = inspect(&mut s, &id).await;
    assert_eq!(first["size_bytes"], 500, "{first}");
    assert_eq!(first["kind"], "folder");
    assert_eq!(inspect(&mut s, &id).await["token"], first["token"]);
    std::fs::write(stray.join("deep").join("c.bin"), vec![3u8; 10]).unwrap();
    let grown = inspect(&mut s, &id).await;
    assert_eq!(grown["size_bytes"], 510);
    assert_ne!(grown["token"], first["token"]);
}

/// Measuring that takes past its limit stops, and the size is left out.
// verifies: FLOW-CLEAN-UP-R12
#[tokio::test(flavor = "multi_thread")]
async fn a_size_not_measured_within_its_limit_is_left_out() {
    let (mut s, _clock) = listing_with(|config| config.measure_limit = Duration::ZERO).await;
    let project = s.project["id"].as_str().unwrap().to_string();
    let stray = worktrees(&s).join("stray");
    std::fs::create_dir_all(&stray).unwrap();
    std::fs::write(stray.join("a.bin"), vec![1u8; 300]).unwrap();
    let inspected = inspect(&mut s, &format!("f:{project}:stray")).await;
    assert!(inspected.get("size_bytes").is_none(), "{inspected}");
    assert!(!inspected["token"].as_str().unwrap().is_empty());
}

/// An id that names nothing, or a thing that is no longer an item, is
/// `not_found`.
// verifies: FLOW-CLEAN-UP-R12
#[tokio::test(flavor = "multi_thread")]
async fn inspecting_what_is_no_longer_an_item_is_not_found() {
    let (mut s, _clock) = listing().await;
    let project = s.project["id"].as_str().unwrap().to_string();
    let workspace = s.workspace().await;
    git(&s.repo, &["branch", "ersa/stray"]);
    git(&s.repo, &["branch", "--quiet", "-D", "ersa/stray"]);
    for id in [
        "nonsense".to_string(),
        ws_id(&workspace),
        format!("b:{project}:ersa/stray"),
        format!("f:{project}:{}", id(&workspace)),
        format!("f:{project}:../repo"),
    ] {
        let error = s.client.error("cleanup.inspect", json!({ "id": id })).await;
        assert_eq!(kind(&error), "not_found", "{id}: {error}");
    }
}

/// The default limit on measuring a folder is 30 seconds.
// verifies: FLOW-CLEAN-UP-R12
#[test]
fn a_folder_is_measured_for_thirty_seconds() {
    assert_eq!(ersa_core::workspace::MEASURE_LIMIT, Duration::from_secs(30));
    let config = CoreConfig::new(std::env::temp_dir()).unwrap();
    assert_eq!(config.measure_limit, Duration::from_secs(30));
}

// -- Sessions that keep a workspace off the list (FLOW-CLEAN-UP-R11) -----

/// A workspace whose sessions are all archived stays off the list while
/// one of them is marked deleting, and while one is dormant, its backend
/// having no adapter in this build.
// verifies: FLOW-CLEAN-UP-R11
#[tokio::test(flavor = "multi_thread")]
async fn a_session_marked_deleting_or_dormant_keeps_its_workspace_off_the_list() {
    let fake = FakeAdapter::new(1);
    let refusal = Arc::clone(&fake.delete_refusal);
    let open = Arc::clone(&fake.open_elsewhere);
    let (mut s, clock) = listing_over(fake, |_| {}).await;
    let marked = s.workspace().await;
    let first = session_in(&mut s, &marked).await;
    let second = session_in(&mut s, &marked).await;
    std::fs::write(folder(&marked).join("notes.txt"), "mine\n").unwrap();
    archive(&mut s, &clock, &first).await;
    archive(&mut s, &clock, &second).await;
    let kept = s.workspace().await;
    let third = session_in(&mut s, &kept).await;
    std::fs::write(folder(&kept).join("notes.txt"), "mine\n").unwrap();
    archive(&mut s, &clock, &third).await;
    let mut both = vec![ws_id(&marked), ws_id(&kept)];
    both.sort();
    assert_eq!(ids(&list(&mut s, 30).await, "workspace"), both);

    // Another process has the sessions open, or may have: running there.
    *open.lock().unwrap() = ersa_adapter::OpenElsewhere::Yes { pids: vec![4242] };
    assert!(ids(&list(&mut s, 30).await, "workspace").is_empty());
    *open.lock().unwrap() = ersa_adapter::OpenElsewhere::Unknown;
    assert!(ids(&list(&mut s, 30).await, "workspace").is_empty());
    *open.lock().unwrap() = ersa_adapter::OpenElsewhere::No;
    assert_eq!(ids(&list(&mut s, 30).await, "workspace"), both);

    // Its record's deletion is refused: marked deleting, `failed`.
    *refusal.lock().unwrap() = Some(ersa_adapter::OpenElsewhere::Yes { pids: vec![4242] });
    let params = json!({ "id": marked["id"] });
    let token = s.client.result("workspace.changes", params).await["token"].clone();
    let delete = json!({ "id": first, "token": token });
    s.client.error("session.delete", delete).await;
    let row = support::listed(&mut s.client, &first).await.unwrap();
    assert_eq!(row["deleting"], "failed", "{row}");
    assert_eq!(
        ids(&list(&mut s, 30).await, "workspace"),
        vec![ws_id(&kept)]
    );
    let error = s
        .client
        .error("cleanup.inspect", json!({ "id": ws_id(&marked) }))
        .await;
    assert_eq!(kind(&error), "not_found", "{error}");

    // The next Core has no adapter for the backend: every session dormant.
    let Setup {
        core,
        client,
        repo,
        project,
        stats,
    } = s;
    drop(client);
    let dir = support::shut_down(core).await;
    let sweeps = Sweeps::Driven(Arc::new(SweepClock::default()));
    let core = start_with_adapters(dir, move |c| c.sweeps = sweeps, Vec::new()).await;
    let client = core.client().await;
    let mut s = Setup {
        core,
        client,
        repo,
        project,
        stats,
    };
    assert!(ids(&list(&mut s, 30).await, "workspace").is_empty());
}

/// [`listing_with`], over `fake`.
async fn listing_over(
    fake: FakeAdapter,
    more: impl FnOnce(&mut CoreConfig),
) -> (Setup, Arc<SweepClock>) {
    let clock = Arc::new(SweepClock::default());
    let stats = Arc::clone(&fake.stats);
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(fake)];
    let sweeps = Sweeps::Driven(Arc::clone(&clock));
    let configure = move |config: &mut CoreConfig| {
        config.sweeps = sweeps;
        more(config);
    };
    let core = start_with_adapters(tempdir(), configure, adapters).await;
    let s = Setup::over(core, stats).await;
    passes(&clock, 1).await;
    (s, clock)
}

/// A workspace git has lost its record of is still checked for a
/// repository of its own, against an empty index as `workspace.changes`
/// counts it: a nested repository the ignore rules do not ignore is one,
/// an ignored one is not, and a folder that is gone holds none.
// verifies: FLOW-CLEAN-UP-R26
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_git_has_no_record_of_is_still_checked_for_a_repository() {
    let (mut s, clock) = listing().await;
    let workspace = s.workspace().await;
    let session = session_in(&mut s, &workspace).await;
    let dir = folder(&workspace);
    std::fs::write(dir.join("notes.txt"), "mine\n").unwrap();
    std::fs::write(dir.join(".gitignore"), "vendor/\n").unwrap();
    nested_repository(&dir.join("vendor"));
    archive(&mut s, &clock, &session).await;
    let record = git(&dir, &["rev-parse", "--absolute-git-dir"]);
    std::fs::remove_dir_all(record.trim()).unwrap();

    let id = ws_id(&workspace);
    let ignored = inspect(&mut s, &id).await;
    assert_eq!(ignored["unrecorded"], true, "{ignored}");
    assert_eq!(ignored["holds_repository"], false, "{ignored}");

    nested_repository(&dir.join("lib"));
    let nested = inspect(&mut s, &id).await;
    assert_eq!(nested["holds_repository"], true, "{nested}");

    std::fs::remove_dir_all(&dir).unwrap();
    let gone = inspect(&mut s, &id).await;
    assert_eq!(gone["holds_repository"], false, "{gone}");
    assert_ne!(gone["token"], nested["token"]);
}
