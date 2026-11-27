//! The sweep (FLOW-CLEAN-UP, The sweep): Core's own pass that puts away the
//! workspaces archived sessions no longer need, against real git
//! repositories and the fake backend, with the sweep's clock in the test's
//! hands (`Sweeps::Driven`): the test says when an hour has passed, and
//! waits for each pass to end.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::OpenElsewhere;
use ersa_core::{Adapter, Gate, SweepClock, Sweeps};
use serde_json::{Value, json};
use support::fake::FakeAdapter;
use support::put_away::*;
use support::sweep::*;
use support::{Client, git};

/// Archives `session`, then waits for the pass the archive asked for.
async fn archive(s: &mut Setup, clock: &SweepClock, session: &str) {
    let before = clock.ended();
    let params = json!({ "id": session, "archived": true });
    s.client.result("session.set_archived", params).await;
    passes(clock, before + 1).await;
}

/// Says an hour has passed, then waits for that pass.
async fn an_hour_passes(clock: &SweepClock) {
    let before = clock.ended();
    clock.hour_passed();
    passes(clock, before + 1).await;
}

/// A workspace with a stopped session in it, and the session.
async fn used(s: &mut Setup) -> (Value, String) {
    let workspace = s.workspace().await;
    let session = s.stopped_session(&workspace).await;
    (workspace, session)
}

/// `workspace` was put away: its folder and git's record gone, its branch
/// kept.
async fn put_away(s: &mut Setup, workspace: &Value) {
    assert_eq!(s.listed(workspace).await["state"], "put_away");
    assert!(!folder(workspace).exists(), "the folder is gone");
    assert!(!listed_by_git(&s.repo, workspace));
    assert!(has_branch(&s.repo, branch(workspace)), "the branch stays");
}

/// `workspace` was left as it was, `inside` still in its folder.
async fn left(s: &mut Setup, workspace: &Value, inside: &str) {
    assert_eq!(s.listed(workspace).await["state"], "ready");
    assert!(folder(workspace).join(inside).exists(), "the folder stays");
    assert!(listed_by_git(&s.repo, workspace));
}

/// No client was told anything of the sweep: `watcher`, connected
/// throughout, got only what its sessions' own messages and archives send,
/// never a notice that belongs to no session. Its answer to a request made
/// now comes after anything Core sent it before.
async fn told_nothing(watcher: &mut Client) {
    watcher.result("core.status", json!({})).await;
    let told = watcher.take_notifications();
    let about_sessions = |told: &Value| {
        let method = told["method"].as_str().unwrap_or_default();
        method == "session.tile" || method == "session.send_progress"
    };
    assert!(told.iter().all(about_sessions), "{told:#?}");
}

// -- After an archive (FLOW-CLEAN-UP-R22, R06, R08) ------------------------

/// A session alone in a clean folder is archived: the pass the archive
/// asks for puts the folder away, keeping the branch, the workspace and
/// the session, and writes it to the diagnostic log. Resuming the session
/// brings the folder back at the same path.
// verifies: FLOW-CLEAN-UP-R22
// verifies: FLOW-CLEAN-UP-R06
// verifies: FLOW-CLEAN-UP-R08
#[tokio::test(flavor = "multi_thread")]
async fn archiving_a_session_alone_in_a_clean_folder_puts_the_folder_away() {
    let log = support::log::Log::installed();
    let (mut s, clock) = swept().await;
    let (workspace, session) = used(&mut s).await;

    archive(&mut s, &clock, &session).await;

    put_away(&mut s, &workspace).await;
    let listed = support::listed(&mut s.client, &session).await;
    assert_eq!(listed.expect("the session stays")["archived"], true);
    let field = format!("workspace={}", id(&workspace));
    assert!(
        log.logged("INFO", "the sweep put the workspace away", &field),
        "{field} not in the log"
    );

    s.client
        .result("session.resume", json!({ "id": session }))
        .await;
    assert!(folder(&workspace).join("README.md").exists(), "it is back");
    assert_eq!(s.listed(&workspace).await["state"], "ready");
}

/// A workspace two sessions use is put away only once both are archived.
// verifies: FLOW-CLEAN-UP-R22
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_two_sessions_use_is_put_away_once_both_are_archived() {
    let (mut s, clock) = swept().await;
    let (workspace, first) = used(&mut s).await;
    let second = s.stopped_session(&workspace).await;

    archive(&mut s, &clock, &first).await;
    left(&mut s, &workspace, "README.md").await;

    archive(&mut s, &clock, &second).await;
    put_away(&mut s, &workspace).await;
}

/// A folder with uncommitted files, or with commits no branch holds, stays;
/// ignored files are not unsaved work and go with the folder.
// verifies: FLOW-CLEAN-UP-R22
// verifies: FLOW-CLEAN-UP-R08
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_with_unsaved_work_stays() {
    let (mut s, clock) = swept().await;

    let (uncommitted, session) = used(&mut s).await;
    std::fs::write(folder(&uncommitted).join("unsaved.txt"), "unsaved\n").unwrap();
    archive(&mut s, &clock, &session).await;
    left(&mut s, &uncommitted, "unsaved.txt").await;

    let (stranded, session) = used(&mut s).await;
    git(&folder(&stranded), &["switch", "--quiet", "--detach"]);
    commit(&folder(&stranded), "detached.txt");
    archive(&mut s, &clock, &session).await;
    left(&mut s, &stranded, "detached.txt").await;

    let (ignored, session) = used(&mut s).await;
    let path = folder(&ignored);
    std::fs::write(path.join(".gitignore"), "*.log\n").unwrap();
    git(&path, &["add", ".gitignore"]);
    git(&path, &["commit", "--quiet", "-m", "ignore logs"]);
    std::fs::write(path.join("build.log"), "ignored\n").unwrap();
    archive(&mut s, &clock, &session).await;
    put_away(&mut s, &ignored).await;
}

/// A session marked deleting keeps the workspace it refers to, archived as
/// it is; once its delete goes through, the next pass puts it away.
// verifies: FLOW-CLEAN-UP-R22
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_a_session_marked_deleting_refers_to_stays() {
    let fake = FakeAdapter::new(1);
    let refusal = Arc::clone(&fake.delete_refusal);
    let (mut s, clock) = swept_with(fake, |_| {}).await;
    let (workspace, first) = used(&mut s).await;
    let second = s.stopped_session(&workspace).await;
    archive(&mut s, &clock, &first).await;
    // Its record's deletion is refused: marked deleting, `failed`.
    *refusal.lock().unwrap() = Some(OpenElsewhere::Yes { pids: vec![4242] });
    let token = changes_token(&mut s, &workspace).await;
    let delete = json!({ "id": first, "token": token });
    s.client.error("session.delete", delete).await;
    let marked = support::listed(&mut s.client, &first).await.unwrap();
    assert_eq!(marked["deleting"], "failed", "{marked}");

    archive(&mut s, &clock, &second).await;
    left(&mut s, &workspace, "README.md").await;

    *refusal.lock().unwrap() = None;
    let token = changes_token(&mut s, &workspace).await;
    let delete = json!({ "id": first, "token": token });
    s.client.result("session.delete", delete).await;
    an_hour_passes(&clock).await;
    put_away(&mut s, &workspace).await;
}

async fn changes_token(s: &mut Setup, workspace: &Value) -> Value {
    let params = json!({ "id": workspace["id"] });
    s.client.result("workspace.changes", params).await["token"].clone()
}

/// A session another process has open keeps its folder, and so does one
/// whose backend can't say.
// verifies: FLOW-CLEAN-UP-R22
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_whose_session_another_process_has_open_stays() {
    let fake = FakeAdapter::new(1);
    let open = Arc::clone(&fake.open_elsewhere);
    let (mut s, clock) = swept_with(fake, |_| {}).await;
    let (workspace, session) = used(&mut s).await;

    *open.lock().unwrap() = OpenElsewhere::Yes { pids: vec![4242] };
    archive(&mut s, &clock, &session).await;
    left(&mut s, &workspace, "README.md").await;

    *open.lock().unwrap() = OpenElsewhere::Unknown;
    an_hour_passes(&clock).await;
    left(&mut s, &workspace, "README.md").await;

    *open.lock().unwrap() = OpenElsewhere::No;
    an_hour_passes(&clock).await;
    put_away(&mut s, &workspace).await;
}

/// A session unarchived after the pass looked at its workspace, here while
/// the pass asks whether another process has it open, keeps its folder:
/// admission decides again as it admits the put-away.
// verifies: FLOW-CLEAN-UP-R22
#[tokio::test(flavor = "multi_thread")]
async fn a_session_unarchived_while_the_pass_judges_its_folder_keeps_it() {
    let fake = FakeAdapter::new(1);
    let registry = Arc::clone(&fake.open_check_gate);
    let (mut s, clock) = swept_with(fake, |_| {}).await;
    let (workspace, session) = used(&mut s).await;
    let before = clock.ended();
    let stats = Arc::clone(&s.stats);
    let asked = move || {
        let arrived = &stats.open_checks_arrived;
        arrived.load(std::sync::atomic::Ordering::SeqCst)
    };
    let asked_before = asked();
    let held = registry.lock().await;

    let params = json!({ "id": session, "archived": true });
    s.client.result("session.set_archived", params).await;
    // At the registry, which the test holds: the pass has read the
    // sessions and found every one archived.
    let deadline = tokio::time::Instant::now() + PASS;
    while asked() == asked_before {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the pass never asked"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let params = json!({ "id": session, "archived": false });
    s.client.result("session.set_archived", params).await;
    drop(held);
    passes(&clock, before + 1).await;

    left(&mut s, &workspace, "README.md").await;
}

/// A workspace no session uses is never put away: it is a leftover's to
/// judge (R23), which leaves it whole while it is younger than an hour.
// verifies: FLOW-CLEAN-UP-R22
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_no_session_uses_is_never_put_away() {
    let (mut s, clock) = swept().await;
    let workspace = s.workspace().await;

    later(&clock, NOT_AN_HOUR).await;

    left(&mut s, &workspace, "README.md").await;
}

/// The project's own folder is never put away.
// verifies: FLOW-CLEAN-UP-R22
#[tokio::test(flavor = "multi_thread")]
async fn a_root_workspace_is_never_put_away() {
    let (mut s, clock) = swept().await;
    let params = json!({ "project_id": s.project["id"] });
    let root = s.client.result("workspace.list", params).await[0].clone();
    assert_eq!(root["kind"], "root");
    let session = s.stopped_session(&root).await;

    archive(&mut s, &clock, &session).await;

    assert_eq!(s.listed(&root).await["state"], "ready");
    assert!(s.repo.join("README.md").exists());
}

// -- At start and every hour (FLOW-CLEAN-UP-R06) ---------------------------

/// An archived session's folder the last Core kept, for a file since
/// deleted: the next Core's pass at start puts it away, except while the
/// session is dormant, its backend having no adapter in that build.
// verifies: FLOW-CLEAN-UP-R06
// verifies: FLOW-CLEAN-UP-R22
#[tokio::test(flavor = "multi_thread")]
async fn the_pass_at_start_puts_away_what_the_last_core_kept() {
    let (mut s, clock) = swept().await;
    let (workspace, session) = used(&mut s).await;
    let unsaved = folder(&workspace).join("unsaved.txt");
    std::fs::write(&unsaved, "unsaved\n").unwrap();
    archive(&mut s, &clock, &session).await;
    left(&mut s, &workspace, "unsaved.txt").await;
    std::fs::remove_file(&unsaved).unwrap();

    let dormant = Arc::new(SweepClock::default());
    let mut s = restarted(s, Vec::new(), &dormant).await;
    passes(&dormant, 1).await;
    left(&mut s, &workspace, "README.md").await;

    let again = Arc::new(SweepClock::default());
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let mut s = restarted(s, adapters, &again).await;
    passes(&again, 1).await;
    put_away(&mut s, &workspace).await;
}

/// What a pass kept, and has since been cleaned, goes on the hour.
// verifies: FLOW-CLEAN-UP-R06
#[tokio::test(flavor = "multi_thread")]
async fn the_sweep_runs_again_each_hour() {
    let (mut s, clock) = swept().await;
    let (workspace, session) = used(&mut s).await;
    let unsaved = folder(&workspace).join("unsaved.txt");
    std::fs::write(&unsaved, "unsaved\n").unwrap();
    archive(&mut s, &clock, &session).await;
    std::fs::remove_file(&unsaved).unwrap();
    left(&mut s, &workspace, "README.md").await;

    an_hour_passes(&clock).await;

    put_away(&mut s, &workspace).await;
}

// -- A repository of its own (FLOW-CLEAN-UP-R22, PROTO-CORE-API-R147) ------

/// Folders that hold a repository of their own stay, `ready`: a submodule
/// checked out, one deinitialized, and a nested repository with nothing
/// uncommitted. One whose submodule was never checked out is put away.
// verifies: FLOW-CLEAN-UP-R22
// verifies: PROTO-CORE-API-R147
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_that_holds_a_repository_of_its_own_stays() {
    let (mut s, clock) = swept().await;
    s.add_submodule();

    let (checked_out, session) = used(&mut s).await;
    check_out_submodules(&folder(&checked_out));
    archive(&mut s, &clock, &session).await;
    left(&mut s, &checked_out, "sub/sub.txt").await;

    let (deinitialized, session) = used(&mut s).await;
    let path = folder(&deinitialized);
    check_out_submodules(&path);
    git(
        &path,
        &["submodule", "deinit", "--quiet", "--all", "--force"],
    );
    archive(&mut s, &clock, &session).await;
    left(&mut s, &deinitialized, "sub").await;

    let (nested, session) = used(&mut s).await;
    nested_repository(&folder(&nested).join("nested"));
    archive(&mut s, &clock, &session).await;
    left(&mut s, &nested, "nested/nested.txt").await;

    let (never, session) = used(&mut s).await;
    archive(&mut s, &clock, &session).await;
    put_away(&mut s, &never).await;
}

// -- When a put-away does not happen (FLOW-CLEAN-UP-R24, PROTO-CORE-API-R124)

/// A put-away git refuses, here over a lock, leaves the workspace `ready`,
/// tells no client, and the next pass judges it again.
// verifies: FLOW-CLEAN-UP-R24
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_put_away_is_judged_again_by_the_next_pass() {
    let (mut s, clock) = swept().await;
    let mut watcher = s.core.client().await;
    let (workspace, session) = used(&mut s).await;
    let path = folder(&workspace).display().to_string();
    git(&s.repo, &["worktree", "lock", &path]);

    archive(&mut s, &clock, &session).await;
    left(&mut s, &workspace, "README.md").await;
    told_nothing(&mut watcher).await;

    git(&s.repo, &["worktree", "unlock", &path]);
    an_hour_passes(&clock).await;
    put_away(&mut s, &workspace).await;
    told_nothing(&mut watcher).await;
}

/// A put-away that had no turn at the project's worktrees in time starts
/// no git work and leaves the workspace for the next pass.
// verifies: PROTO-CORE-API-R124
// verifies: FLOW-CLEAN-UP-R24
#[tokio::test(flavor = "multi_thread")]
async fn a_put_away_with_no_turn_in_time_is_left_for_the_next_pass() {
    let (mut s, clock) = swept_with(FakeAdapter::new(1), |config| {
        config.worktree_turn_wait = Duration::from_millis(300);
    })
    .await;
    let (workspace, session) = used(&mut s).await;
    let core = s.core.core();
    let project = s.project["id"].as_str().unwrap().to_string();
    let wait = Duration::from_secs(30);
    let turn = ersa_core::workspace::project_turn(&core.admission, &project, wait)
        .await
        .expect("the test has the turn");

    archive(&mut s, &clock, &session).await;
    left(&mut s, &workspace, "README.md").await;

    drop(turn);
    an_hour_passes(&clock).await;
    put_away(&mut s, &workspace).await;
}

/// A put-away that fails after the workspace was stored `put_away`, its
/// try at the folder stuck past its limit: the workspace stays `put_away`,
/// and the pass goes no further while that try holds its place (R06). Once
/// the try ends, having deleted nothing, the pass judges the next
/// workspace. No later pass tries the put-away again or removes anything at
/// its path, a file written there since included, and no client is told.
// verifies: FLOW-CLEAN-UP-R24
// verifies: FLOW-CLEAN-UP-R06
#[tokio::test(flavor = "multi_thread")]
async fn a_put_away_that_failed_part_way_is_never_tried_again() {
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let clock = Arc::new(SweepClock::default());
    let sweeps = Sweeps::Driven(Arc::clone(&clock));
    let wait = ersa_core::workspace::WORKTREE_TURN_WAIT;
    let mut s = stuck_folder_setup_with(&gate, wait, move |config| config.sweeps = sweeps).await;
    passes(&clock, 1).await;
    let mut watcher = s.core.client().await;
    // Kept at their archive by a file each, then cleaned: the next pass
    // judges `first`, then `second`.
    let (first, session) = used(&mut s).await;
    let (second, other) = used(&mut s).await;
    for (workspace, session) in [(&first, &session), (&second, &other)] {
        let unsaved = folder(workspace).join("unsaved.txt");
        std::fs::write(&unsaved, "unsaved\n").unwrap();
        archive(&mut s, &clock, session).await;
    }
    std::fs::remove_file(folder(&first).join("unsaved.txt")).unwrap();
    let before = clock.ended();

    // git drops its record and fails, and the try at the folder sticks.
    let log = support::log::Log::installed();
    clock.hour_passed();
    reaches_the_try(&gate, &clock, before).await;
    let failed = format!("workspace={}", id(&first));
    put_away_failed(&log, &failed).await;
    assert_eq!(s.listed(&first).await["state"], "put_away");
    // The try ran out, and the put-away with it; the pass, which would
    // judge `second` in well under a second and end, waits for the try to
    // let go.
    let ended = tokio::time::timeout(Duration::from_secs(5), clock.until_ended(before + 1));
    assert!(ended.await.is_err(), "the pass went on beside a stuck try");

    gate.go();
    passes(&clock, before + 1).await;
    assert!(
        folder(&first).join("README.md").exists(),
        "the try deleted nothing"
    );
    left(&mut s, &second, "unsaved.txt").await;

    // Anything written at its path since stays: every try at a folder
    // stops at the gate, so a pass that ends began none.
    std::fs::write(folder(&first).join("since.txt"), "written since\n").unwrap();
    an_hour_passes(&clock).await;
    assert!(folder(&first).join("since.txt").exists());
    assert_eq!(s.listed(&first).await["state"], "put_away");
    told_nothing(&mut watcher).await;
}

// -- Leftovers (FLOW-CLEAN-UP-R23, R08, R25) --------------------------------

/// Just short of an hour: a workspace made at the start of a test is still
/// younger than an hour once this has passed, however slow the test.
const NOT_AN_HOUR: Duration = Duration::from_secs(59 * 60);

/// `workspace` was removed: gone from the list, and its folder, git's
/// record of it and its branch with it.
async fn removed(s: &mut Setup, workspace: &Value) {
    assert_eq!(
        s.listed(workspace).await,
        Value::Null,
        "the workspace is gone"
    );
    assert!(!folder(workspace).exists(), "the folder is gone");
    assert!(!listed_by_git(&s.repo, workspace), "git's record is gone");
    assert!(
        !has_branch(&s.repo, branch(workspace)),
        "the branch is gone"
    );
}

/// A workspace whose start was cut off as `workspace.create` answered, the
/// link that asked for it dropped: the sweep leaves it while it is younger
/// than an hour, then removes it with its folder, git's record and its
/// branch, and the project's folder under the worktrees root, now empty,
/// and writes the removal to the diagnostic log.
// verifies: FLOW-CLEAN-UP-R23
// verifies: FLOW-CLEAN-UP-R08
#[tokio::test(flavor = "multi_thread")]
async fn a_leftover_that_holds_nothing_goes_once_an_hour_has_passed() {
    let log = support::log::Log::installed();
    let (mut s, clock) = swept().await;
    let mut cut_off = s.core.client().await;
    let params = json!({ "project_id": s.project["id"] });
    let workspace = cut_off.result("workspace.create", params).await;
    drop(cut_off);
    let project_folder = folder(&workspace).parent().unwrap().to_path_buf();

    later(&clock, NOT_AN_HOUR).await;
    left(&mut s, &workspace, "README.md").await;
    assert!(has_branch(&s.repo, branch(&workspace)));

    later(&clock, Duration::from_secs(60)).await;
    removed(&mut s, &workspace).await;
    assert!(!project_folder.exists(), "the empty project folder is gone");
    let field = format!("workspace={}", id(&workspace));
    let removal = "the sweep removed a leftover workspace";
    assert!(
        log.logged("INFO", removal, &field),
        "{field} not in the log"
    );
}

/// The diagnostic log's line for a leftover kept, as R08 asks.
const KEPT: &str = "the sweep kept a leftover workspace";

/// `workspace`, a leftover, is kept as it was, `inside` still there, and its
/// keeping was written to the diagnostic log once, saying `why`.
async fn kept(s: &mut Setup, log: &support::log::Log, workspace: &Value, inside: &str, why: &str) {
    left(s, workspace, inside).await;
    assert!(has_branch(&s.repo, branch(workspace)), "the branch stays");
    let field = format!("workspace={}", id(workspace));
    assert_eq!(log.count("INFO", KEPT, &field), 1, "{field}: one line");
    assert!(log.logged("INFO", why, &field), "{field}: says {why:?}");
}

/// Leftovers that hold something stay, each written to the diagnostic log
/// once with why: uncommitted files, ignored files, a commit no other branch
/// holds, on its branch or on no branch, a repository of its own, and a
/// folder git has no record of, whose contents can't be checked.
// verifies: FLOW-CLEAN-UP-R23
// verifies: FLOW-CLEAN-UP-R08
#[tokio::test(flavor = "multi_thread")]
async fn a_leftover_that_holds_anything_stays() {
    let log = support::log::Log::installed();
    let (mut s, clock) = swept().await;

    let uncommitted = s.workspace().await;
    std::fs::write(folder(&uncommitted).join("unsaved.txt"), "unsaved\n").unwrap();

    let ignored = s.workspace().await;
    let path = folder(&ignored);
    std::fs::write(path.join(".gitignore"), "*.log\n").unwrap();
    git(&path, &["add", ".gitignore"]);
    git(&path, &["commit", "--quiet", "-m", "ignore logs"]);
    // Shared with main, so only the ignored file is left to keep it.
    git(
        &s.repo,
        &["merge", "--quiet", "--ff-only", branch(&ignored)],
    );
    std::fs::write(path.join("build.log"), "ignored\n").unwrap();

    let unshared = s.workspace().await;
    commit(&folder(&unshared), "only-here.txt");

    let detached = s.workspace().await;
    git(&folder(&detached), &["switch", "--quiet", "--detach"]);
    commit(&folder(&detached), "detached.txt");

    let nested = s.workspace().await;
    // Its commit is held by no remote of its own: one uncommitted entry.
    nested_repository(&folder(&nested).join("nested"));

    let unrecorded = s.workspace().await;
    let link = std::fs::read_to_string(folder(&unrecorded).join(".git")).unwrap();
    let record = link.trim().trim_start_matches("gitdir:").trim().to_string();
    std::fs::remove_dir_all(record).unwrap();

    an_hour_passes(&clock).await;
    an_hour_passes(&clock).await;

    kept(
        &mut s,
        &log,
        &uncommitted,
        "unsaved.txt",
        "uncommitted files",
    )
    .await;
    kept(&mut s, &log, &ignored, "build.log", "ignored files: 1").await;
    kept(
        &mut s,
        &log,
        &unshared,
        "only-here.txt",
        "commits no other branch holds",
    )
    .await;
    kept(
        &mut s,
        &log,
        &detached,
        "detached.txt",
        "commits no other branch holds",
    )
    .await;
    kept(
        &mut s,
        &log,
        &nested,
        "nested/nested.txt",
        "uncommitted files",
    )
    .await;
    // git no longer lists it, so `left`'s look at git's list does not hold.
    assert!(folder(&unrecorded).join("README.md").exists());
    assert_eq!(s.listed(&unrecorded).await["state"], "ready");
    let field = format!("workspace={}", id(&unrecorded));
    assert_eq!(log.count("INFO", KEPT, &field), 1);
    assert!(log.logged("INFO", "git has no record of it", &field));
}

/// A leftover whose folder holds a repository of its own and nothing else
/// stays (PROTO-CORE-API-R147): a submodule checked out.
// verifies: FLOW-CLEAN-UP-R23
#[tokio::test(flavor = "multi_thread")]
async fn a_leftover_that_holds_a_repository_of_its_own_stays() {
    let log = support::log::Log::installed();
    let (mut s, clock) = swept().await;
    s.add_submodule();
    let workspace = s.workspace().await;
    check_out_submodules(&folder(&workspace));

    an_hour_passes(&clock).await;

    let why = "its folder holds a repository of its own";
    kept(&mut s, &log, &workspace, "sub/sub.txt", why).await;
}

/// A workspace a session refers to is no leftover, whatever the session is
/// doing: here one stopped, one marked deleting whose delete failed, and
/// one dormant after a restart, its backend having no adapter in that
/// build. No workspace and no session goes.
// verifies: FLOW-CLEAN-UP-R23
// verifies: FLOW-CLEAN-UP-R08
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_a_session_refers_to_is_no_leftover() {
    let failing = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let transport = Arc::new(FailingWhile {
        failing: Arc::clone(&failing),
        inner: ersa_core::transport::LocalTreeTransport::for_commands(),
    });
    let (mut s, clock) = swept_with(FakeAdapter::new(1), move |config| {
        config.command_transport = Some(ersa_core::CommandTransport(transport));
    })
    .await;
    let (stopped, session) = used(&mut s).await;
    let (marked, deleting) = used(&mut s).await;
    // Its delete fails at the folder, before git touches it: the session
    // stays marked deleting, its folder and git's record whole.
    failing.store(true, std::sync::atomic::Ordering::SeqCst);
    let token = changes_token(&mut s, &marked).await;
    let delete = json!({ "id": deleting, "token": token });
    s.client.error("session.delete", delete).await;
    failing.store(false, std::sync::atomic::Ordering::SeqCst);
    let row = support::listed(&mut s.client, &deleting).await.unwrap();
    assert_eq!(row["deleting"], "failed", "{row}");

    an_hour_passes(&clock).await;

    left(&mut s, &stopped, "README.md").await;
    assert_ne!(s.listed(&marked).await, Value::Null, "the workspace stays");
    assert!(
        folder(&marked).join("README.md").exists(),
        "its folder stays"
    );
    assert!(has_branch(&s.repo, branch(&marked)), "its branch stays");
    for session in [&session, &deleting] {
        let listed = support::listed(&mut s.client, session).await;
        assert!(listed.is_some(), "{session} is not deleted");
    }

    let dormant = Arc::new(SweepClock::default());
    dormant.time_passed(Duration::from_secs(2 * 60 * 60));
    let mut s = restarted(s, Vec::new(), &dormant).await;
    passes(&dormant, 1).await;
    left(&mut s, &stopped, "README.md").await;
    // Not listed while dormant: its row is what stays.
    let rows = s.core.core().store.sessions().await.unwrap();
    assert!(rows.iter().any(|row| row.id == session), "not deleted");
}

/// A leftover whose folder is missing with the folder above it, as on a
/// drive that is not plugged in, can't be checked: it stays, with git's
/// record of it and its branch.
// verifies: FLOW-CLEAN-UP-R08
// verifies: FLOW-CLEAN-UP-R23
#[tokio::test(flavor = "multi_thread")]
async fn a_leftover_out_of_reach_stays() {
    let log = support::log::Log::installed();
    let (mut s, clock) = swept().await;
    let workspace = s.workspace().await;
    let above = folder(&workspace).parent().unwrap().to_path_buf();
    let away = above.with_extension("unplugged");
    std::fs::rename(&above, &away).unwrap();

    an_hour_passes(&clock).await;

    assert_eq!(s.listed(&workspace).await["state"], "ready");
    assert!(listed_by_git(&s.repo, &workspace), "git's record stays");
    assert!(has_branch(&s.repo, branch(&workspace)), "the branch stays");
    let field = format!("workspace={}", id(&workspace));
    assert!(log.logged("INFO", "its folder can't be reached", &field));
    // Plugged in again, it is whole.
    std::fs::rename(&away, &above).unwrap();
    left(&mut s, &workspace, "README.md").await;
}

/// A put-away workspace no session uses, here its last session removed (a
/// delete would take the workspace), and `put_away` still.
async fn put_away_alone(s: &mut Setup, clock: &SweepClock) -> Value {
    let (workspace, session) = used(s).await;
    archive(s, clock, &session).await;
    put_away(s, &workspace).await;
    let remove = json!({ "id": session });
    s.client.result("session.remove", remove).await;
    assert_eq!(s.listed(&workspace).await["state"], "put_away");
    workspace
}

/// A put-away workspace no session uses is a leftover with no folder: its
/// branch and the workspace go. One with something standing at its path is
/// what a failed put-away left, no leftover, and stays with what is there.
// verifies: FLOW-CLEAN-UP-R23
#[tokio::test(flavor = "multi_thread")]
async fn a_put_away_workspace_no_session_uses_goes_unless_something_is_at_its_path() {
    let (mut s, clock) = swept().await;
    let gone = put_away_alone(&mut s, &clock).await;
    let remains = put_away_alone(&mut s, &clock).await;
    std::fs::create_dir_all(folder(&remains)).unwrap();
    std::fs::write(folder(&remains).join("left.txt"), "left\n").unwrap();

    an_hour_passes(&clock).await;

    assert_eq!(s.listed(&gone).await, Value::Null, "the workspace is gone");
    assert!(!has_branch(&s.repo, branch(&gone)), "its branch is gone");
    assert_eq!(s.listed(&remains).await["state"], "put_away");
    assert!(folder(&remains).join("left.txt").exists());
    assert!(has_branch(&s.repo, branch(&remains)));
}

/// A put-away workspace no session uses with git's record of a worktree at
/// its path, the folder gone, is what a failed put-away left too: no
/// leftover. Alone in its project here: any removal there prunes git's
/// stale records, this one with them, after which it is a leftover.
// verifies: FLOW-CLEAN-UP-R23
#[tokio::test(flavor = "multi_thread")]
async fn a_put_away_workspace_with_git_s_record_at_its_path_is_no_leftover() {
    let (mut s, clock) = swept().await;
    let recorded = put_away_alone(&mut s, &clock).await;
    let path = folder(&recorded).display().to_string();
    let add = ["worktree", "add", "--quiet", &path, branch(&recorded)];
    git(&s.repo, &add);
    std::fs::remove_dir_all(folder(&recorded)).unwrap();

    an_hour_passes(&clock).await;

    assert_eq!(s.listed(&recorded).await["state"], "put_away");
    assert!(listed_by_git(&s.repo, &recorded), "git's record stays");
    assert!(has_branch(&s.repo, branch(&recorded)));
}

/// A leftover whose folder was deleted by hand, git's record of it left
/// stale: the sweep removes the record, the branch and the workspace.
// verifies: FLOW-CLEAN-UP-R23
#[tokio::test(flavor = "multi_thread")]
async fn a_leftover_whose_folder_is_gone_takes_git_s_stale_record_with_it() {
    let (mut s, clock) = swept().await;
    let workspace = s.workspace().await;
    std::fs::remove_dir_all(folder(&workspace)).unwrap();
    assert!(
        listed_by_git(&s.repo, &workspace),
        "git still has its record"
    );

    an_hour_passes(&clock).await;

    removed(&mut s, &workspace).await;
}

/// An ignored file written into a leftover after the sweep counted it,
/// while its removal waits for the project's turn: git would remove it
/// unasked, so the removal judges the leftover again in the turn and keeps
/// it.
// verifies: FLOW-CLEAN-UP-R23
// verifies: FLOW-CLEAN-UP-R08
#[tokio::test(flavor = "multi_thread")]
async fn a_leftover_written_to_while_its_removal_waits_stays() {
    let (mut s, clock) = swept().await;
    let workspace = s.workspace().await;
    let path = folder(&workspace);
    std::fs::write(path.join(".gitignore"), "*.log\n").unwrap();
    git(&path, &["add", ".gitignore"]);
    git(&path, &["commit", "--quiet", "-m", "ignore logs"]);
    git(
        &s.repo,
        &["merge", "--quiet", "--ff-only", branch(&workspace)],
    );
    let core = s.core.core();
    let project = s.project["id"].as_str().unwrap().to_string();
    let turn = ersa_core::workspace::project_turn(&core.admission, &project, PASS)
        .await
        .expect("the test has the turn");
    let before = clock.ended();
    clock.hour_passed();
    // Admitted, counted and waiting for the turn the test holds.
    let removing = || {
        let state = core.admission.snapshot();
        let phase = state.workspace(id(&workspace)).map(|w| w.phase);
        matches!(
            phase,
            Some(ersa_core::admission::WorkspacePhase::Removing { .. })
        )
    };
    let deadline = tokio::time::Instant::now() + PASS;
    while !removing() {
        assert!(tokio::time::Instant::now() < deadline, "never removing");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    std::fs::write(path.join("since.log"), "since\n").unwrap();
    drop(turn);
    passes(&clock, before + 1).await;

    left(&mut s, &workspace, "since.log").await;
}

/// The sweep's removal goes on to the folder when git dropped its record
/// and failed (PROTO-CORE-API-R112, R122), and deletes the branch.
// verifies: PROTO-CORE-API-R122
#[tokio::test(flavor = "multi_thread")]
async fn the_sweep_removes_a_leftover_as_every_removal_does() {
    let drops = Arc::new(support::deleting::FailingGit::new(
        support::deleting::Removal::DropsRecordThenFails,
        None,
    ));
    let (mut s, clock) = swept_with(FakeAdapter::new(1), move |config| {
        config.command_transport = Some(ersa_core::CommandTransport(drops));
    })
    .await;
    let workspace = s.workspace().await;

    an_hour_passes(&clock).await;

    removed(&mut s, &workspace).await;
}

/// A removal that had no turn at the project's worktrees in time starts no
/// git work and leaves the leftover for the next pass.
// verifies: PROTO-CORE-API-R124
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_with_no_turn_in_time_is_left_for_the_next_pass() {
    let (mut s, clock) = swept_with(FakeAdapter::new(1), |config| {
        config.worktree_turn_wait = Duration::from_millis(300);
    })
    .await;
    let workspace = s.workspace().await;
    let core = s.core.core();
    let project = s.project["id"].as_str().unwrap().to_string();
    let wait = Duration::from_secs(30);
    let turn = ersa_core::workspace::project_turn(&core.admission, &project, wait)
        .await
        .expect("the test has the turn");

    an_hour_passes(&clock).await;
    left(&mut s, &workspace, "README.md").await;
    assert!(has_branch(&s.repo, branch(&workspace)));

    drop(turn);
    an_hour_passes(&clock).await;
    removed(&mut s, &workspace).await;
}

/// git's `worktree remove` fails while `failing` is set, before it touches
/// anything; every other command runs as the local transport runs it.
struct FailingWhile {
    failing: Arc<std::sync::atomic::AtomicBool>,
    inner: ersa_core::transport::LocalTreeTransport,
}

#[ersa_adapter::async_trait]
impl ersa_adapter::Transport for FailingWhile {
    async fn spawn(
        &self,
        mut spec: ersa_adapter::ProcessSpec,
    ) -> std::io::Result<Box<dyn ersa_adapter::Process>> {
        let removes = spec.args.windows(2).any(|w| w == ["worktree", "remove"]);
        if removes && self.failing.load(std::sync::atomic::Ordering::SeqCst) {
            spec.args.push("--no-such-option".into());
        }
        self.inner.spawn(spec).await
    }
}

/// A removal that fails tells no client, and the next pass judges the
/// leftover again and removes it.
// verifies: FLOW-CLEAN-UP-R25
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_that_failed_is_judged_again_by_the_next_pass() {
    let failing = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let transport = Arc::new(FailingWhile {
        failing: Arc::clone(&failing),
        inner: ersa_core::transport::LocalTreeTransport::for_commands(),
    });
    let (mut s, clock) = swept_with(FakeAdapter::new(1), move |config| {
        config.command_transport = Some(ersa_core::CommandTransport(transport));
    })
    .await;
    let mut watcher = s.core.client().await;
    let workspace = s.workspace().await;

    an_hour_passes(&clock).await;
    assert_eq!(s.listed(&workspace).await["state"], "error");
    assert!(folder(&workspace).join("README.md").exists());
    told_nothing(&mut watcher).await;

    failing.store(false, std::sync::atomic::Ordering::SeqCst);
    an_hour_passes(&clock).await;
    removed(&mut s, &workspace).await;
    told_nothing(&mut watcher).await;
}

/// A removal whose try at the folder is stuck past its limit: while the try
/// holds, nothing else removes the workspace or its folder, and the pass
/// goes no further. Once the try ends, having deleted nothing, git has no
/// record of the folder, so the next pass can't check it and keeps it.
// verifies: PROTO-CORE-API-R131
// verifies: FLOW-CLEAN-UP-R25
// verifies: FLOW-CLEAN-UP-R08
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_whose_try_is_stuck_holds_the_workspace() {
    let log = support::log::Log::installed();
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let clock = Arc::new(SweepClock::default());
    let sweeps = Sweeps::Driven(Arc::clone(&clock));
    let wait = ersa_core::workspace::WORKTREE_TURN_WAIT;
    let mut s = stuck_folder_setup_with(&gate, wait, move |config| config.sweeps = sweeps).await;
    passes(&clock, 1).await;
    let mut watcher = s.core.client().await;
    let workspace = s.workspace().await;
    let before = clock.ended();

    clock.hour_passed();
    reaches_the_try(&gate, &clock, before).await;
    let field = format!("workspace={}", id(&workspace));
    let deadline = tokio::time::Instant::now() + PASS;
    while !log.logged("INFO", KEPT, &field) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the removal never failed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let params = json!({ "id": workspace["id"] });
    let refused = s.client.error("workspace.remove", params).await;
    assert_eq!(refused["data"]["kind"], "invalid_state", "{refused}");
    let ended = tokio::time::timeout(Duration::from_secs(5), clock.until_ended(before + 1));
    assert!(ended.await.is_err(), "the pass went on beside a stuck try");

    gate.go();
    passes(&clock, before + 1).await;
    assert!(
        folder(&workspace).join("README.md").exists(),
        "the try deleted nothing"
    );

    an_hour_passes(&clock).await;
    assert!(folder(&workspace).join("README.md").exists());
    assert_eq!(s.listed(&workspace).await["state"], "error");
    // Written the first time it was kept, when its removal failed.
    assert_eq!(log.count("INFO", KEPT, &field), 1);
    told_nothing(&mut watcher).await;
}

/// Waits until the put-away of the workspace `field` names has answered,
/// failed part way, as it writes to the diagnostic log.
async fn put_away_failed(log: &support::log::Log, field: &str) {
    let deadline = tokio::time::Instant::now() + PASS;
    while !log.warned("putting the workspace away failed part way", field) {
        assert!(tokio::time::Instant::now() < deadline, "it never failed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// While the sweep counts what a workspace holds, requests that need no
/// turn it holds go on: a count and a delete in another workspace of the
/// same project answer.
// verifies: FLOW-CLEAN-UP-R06
#[tokio::test(flavor = "multi_thread")]
async fn no_request_waits_for_the_sweep() {
    let stall = support::Stall::new();
    let transport = Arc::new(support::StallingTransport {
        stall: Arc::clone(&stall),
        inner: ersa_core::transport::LocalTreeTransport::for_commands(),
    });
    let (mut s, clock) = swept_with(FakeAdapter::new(1), move |config| {
        config.command_transport = Some(ersa_core::CommandTransport(transport));
    })
    .await;
    let (workspace, session) = used(&mut s).await;
    let (other, beside) = used(&mut s).await;
    let before = clock.ended();

    // The sweep's first git call, counting `workspace`, does not answer.
    stall.arm();
    let params = json!({ "id": session, "archived": true });
    s.client.result("session.set_archived", params).await;
    stall.reached().await;

    let token = changes_token(&mut s, &other).await;
    let delete = json!({ "id": beside, "token": token });
    let deleted = s.client.result("session.delete", delete).await;
    assert_eq!(deleted["workspace_deleted"], true, "{deleted}");
    assert_eq!(clock.ended(), before, "the pass is still under way");

    stall.go();
    passes(&clock, before + 1).await;
    put_away(&mut s, &workspace).await;
}
