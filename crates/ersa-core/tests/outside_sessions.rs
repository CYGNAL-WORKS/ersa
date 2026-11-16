//! Sessions started in the backend's own tools (FLOW-RESUME-SESSION, step 5),
//! through the Core API with the fake adapter (`support/fake.rs`), whose
//! records, and the folders it files them under, outlive a Core as Claude
//! Code's do: listed in the workspace whose folder they ran in, kept under one
//! id with their arrangement, left out once their record is gone, and
//! forgotten with their workspace or project.

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use ersa_core::transport::LocalTreeTransport;
use ersa_core::{Adapter, CommandTransport};
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter, Filed, FiledRecord, Records};
use support::{Client, Stall, StallingTransport, TestCore, project, start_with_adapters, tempdir};

/// The fake backend's records and where it files them, shared by every Core
/// of a test.
#[derive(Default, Clone)]
struct Backend {
    records: Records,
    filed: Filed,
    /// Holds every folder listing, once read, while a test holds it.
    list_gate: Arc<tokio::sync::Mutex<()>>,
    stats: Arc<fake::Stats>,
}

impl Backend {
    fn adapter(&self) -> Arc<dyn Adapter> {
        self.adapter_from(FakeAdapter::with_records(Arc::clone(&self.records)))
    }

    /// The fake `adapter` keeping this backend's records.
    fn adapter_from(&self, mut adapter: FakeAdapter) -> Arc<dyn Adapter> {
        adapter.records = Arc::clone(&self.records);
        adapter.filed = Arc::clone(&self.filed);
        adapter.list_gate = Arc::clone(&self.list_gate);
        adapter.stats = Arc::clone(&self.stats);
        Arc::new(adapter)
    }

    /// A record of a session started in the backend's own tools, whose first
    /// prompt is `prompt`, filed under `under` and naming `cwd` as its own.
    fn record(&self, id: &str, prompt: &str, under: &str, cwd: &str) {
        self.records
            .lock()
            .unwrap()
            .insert(id.into(), vec![fake::recorded_user("m-1", prompt)]);
        self.file(id, under, cwd);
    }

    fn file(&self, id: &str, under: &str, cwd: &str) {
        self.filed.lock().unwrap().insert(
            id.into(),
            FiledRecord {
                under: under.into(),
                cwd: cwd.into(),
            },
        );
    }

    fn has_record(&self, id: &str) -> bool {
        self.records.lock().unwrap().contains_key(id)
    }
}

async fn start(backend: &Backend, dir: tempfile::TempDir) -> TestCore {
    start_with_adapters(dir, |_| {}, vec![backend.adapter()]).await
}

/// Stops `core` and starts another over the same data directory.
async fn restart(core: TestCore, backend: &Backend) -> TestCore {
    let TestCore { dir, task, .. } = core;
    task.abort();
    let _ = task.await;
    start(backend, dir).await
}

/// A project's workspaces, root first.
async fn workspaces(client: &mut Client, project_id: &Value) -> Vec<Value> {
    client
        .result("workspace.list", json!({ "project_id": project_id }))
        .await
        .as_array()
        .unwrap()
        .clone()
}

fn path(workspace: &Value) -> &str {
    workspace["path"].as_str().unwrap()
}

async fn sessions(client: &mut Client) -> Vec<Value> {
    client
        .result("session.list", json!({}))
        .await
        .as_array()
        .unwrap()
        .clone()
}

/// The listed session whose first prompt titles it `title`.
fn titled<'a>(sessions: &'a [Value], title: &str) -> Option<&'a Value> {
    sessions.iter().find(|s| s["title"] == title)
}

// verifies: FLOW-RESUME-SESSION-R12
#[tokio::test(flavor = "multi_thread")]
async fn the_list_holds_every_session_recorded_for_a_projects_folders_in_its_workspace() {
    let backend = Backend::default();
    let core = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let project = project(&mut client, core.dir.path()).await;
    client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    let [root, isolated] = <[Value; 2]>::try_from(workspaces(&mut client, &project["id"]).await)
        .expect("a root and an isolated workspace");

    // One session started in the backend's own tools in each folder.
    backend.record("rec-root", "in the root", path(&root), path(&root));
    backend.record(
        "rec-isolated",
        "in the isolated",
        path(&isolated),
        path(&isolated),
    );
    // One the backend files with the root's records whose own folder is
    // another (Claude Code's folder names are lossy).
    let other = format!("{}-other", path(&root));
    backend.record("rec-other", "in another folder", path(&root), &other);
    // A session Ersa started, whose record is filed with the root's too.
    let ersa = client
        .result(
            "session.create",
            json!({ "workspace_id": root["id"], "backend": fake::BACKEND }),
        )
        .await;
    client
        .result(
            "session.send",
            json!({ "id": ersa["id"], "text": "from ersa" }),
        )
        .await;
    client
        .result("session.stop", json!({ "id": ersa["id"] }))
        .await;
    let ersa_record = {
        let records = backend.records.lock().unwrap();
        records
            .keys()
            .find(|id| {
                id.as_str() != "rec-root"
                    && id.as_str() != "rec-isolated"
                    && id.as_str() != "rec-other"
            })
            .cloned()
            .expect("the record of Ersa's session")
    };
    backend.file(&ersa_record, path(&root), path(&root));

    let listed = sessions(&mut client).await;
    assert_eq!(listed.len(), 3, "{listed:#?}");
    let in_root = titled(&listed, "in the root").expect("the root's session");
    assert_eq!(in_root["workspace_id"], root["id"]);
    assert_eq!(in_root["backend"], fake::BACKEND);
    assert_eq!(in_root["host_id"], "local");
    assert_eq!(in_root["created_at"], "2026-11-01T00:00:00.000Z");
    let in_isolated = titled(&listed, "in the isolated").expect("the isolated workspace's session");
    assert_eq!(in_isolated["workspace_id"], isolated["id"]);
    assert!(titled(&listed, "in another folder").is_none());
    // Ersa's own session is listed once, as it was.
    assert_eq!(listed.iter().filter(|s| s["id"] == ersa["id"]).count(), 1);
    // A session listed for the first time has none of the arrangement set.
    for found in [in_root, in_isolated] {
        assert_eq!(found["pinned"], false);
        assert_eq!(found["archived"], false);
        assert!(found.get("section_id").is_none());
        assert!(found.get("mode").is_none_or(Value::is_null));
    }
}

// verifies: FLOW-RESUME-SESSION-R05
#[tokio::test(flavor = "multi_thread")]
async fn a_session_found_in_the_record_keeps_its_id_and_arrangement_across_restarts() {
    let backend = Backend::default();
    let core = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let project = project(&mut client, core.dir.path()).await;
    let root = workspaces(&mut client, &project["id"]).await.remove(0);
    backend.record("rec-a", "found", path(&root), path(&root));

    let first = sessions(&mut client).await;
    let id = titled(&first, "found").expect("listed")["id"].clone();
    let again = sessions(&mut client).await;
    assert_eq!(again.len(), 1);
    assert_eq!(again[0]["id"], id);

    let section = client
        .result("section.create", json!({ "name": "Later" }))
        .await;
    client
        .result(
            "session.arrange",
            json!({ "id": id, "pinned": true, "section_id": section["id"] }),
        )
        .await;
    client
        .result(
            "session.set_archived",
            json!({ "id": id, "archived": true }),
        )
        .await;
    drop(client);

    let core = restart(core, &backend).await;
    let mut client = core.client().await;
    let listed = sessions(&mut client).await;
    assert_eq!(listed.len(), 1, "{listed:#?}");
    assert_eq!(listed[0]["id"], id);
    assert_eq!(listed[0]["title"], "found");
    assert_eq!(listed[0]["pinned"], true);
    assert_eq!(listed[0]["section_id"], section["id"]);
    assert_eq!(listed[0]["archived"], true);
}

// verifies: FLOW-RESUME-SESSION-R06
#[tokio::test(flavor = "multi_thread")]
async fn a_found_session_whose_record_is_gone_leaves_the_list_and_one_ersa_started_stays() {
    let backend = Backend::default();
    let core = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let project = project(&mut client, core.dir.path()).await;
    let root = workspaces(&mut client, &project["id"]).await.remove(0);
    backend.record("rec-a", "found", path(&root), path(&root));
    let found = titled(&sessions(&mut client).await, "found").expect("listed")["id"].clone();
    // A new session Ersa started has no record until its first message.
    let ersa = client
        .result(
            "session.create",
            json!({ "workspace_id": root["id"], "backend": fake::BACKEND }),
        )
        .await;
    client
        .result("session.stop", json!({ "id": ersa["id"] }))
        .await;

    // The backend's retention removes the found session's record. Ersa's
    // session has none either, as a new session has none until its first
    // message, though the fake made an empty one when it started.
    {
        let mut records = backend.records.lock().unwrap();
        records.clear();
    }
    let listed = sessions(&mut client).await;
    assert!(listed.iter().all(|s| s["id"] != found), "{listed:#?}");
    assert!(listed.iter().any(|s| s["id"] == ersa["id"]));
    drop(client);

    // From then on, after a restart too.
    let core = restart(core, &backend).await;
    let mut client = core.client().await;
    let listed = sessions(&mut client).await;
    assert_eq!(listed.len(), 1, "{listed:#?}");
    assert_eq!(listed[0]["id"], ersa["id"]);
}

// verifies: FLOW-RESUME-SESSION-R13
#[tokio::test(flavor = "multi_thread")]
async fn removing_a_workspace_or_project_forgets_its_found_sessions_and_leaves_their_records() {
    let backend = Backend::default();
    let core = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let project = project(&mut client, core.dir.path()).await;
    client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    let [root, isolated] = <[Value; 2]>::try_from(workspaces(&mut client, &project["id"]).await)
        .expect("a root and an isolated workspace");
    backend.record("rec-root", "in the root", path(&root), path(&root));
    backend.record(
        "rec-isolated",
        "in the isolated",
        path(&isolated),
        path(&isolated),
    );
    assert_eq!(sessions(&mut client).await.len(), 2);

    client
        .result("workspace.remove", json!({ "id": isolated["id"] }))
        .await;
    assert!(backend.has_record("rec-isolated"));
    let listed = sessions(&mut client).await;
    assert_eq!(listed.len(), 1, "{listed:#?}");
    assert_eq!(listed[0]["title"], "in the root");

    client
        .result("project.remove", json!({ "id": project["id"] }))
        .await;
    assert!(backend.has_record("rec-root"));
    assert!(sessions(&mut client).await.is_empty());

    // The folder added again lists its session again.
    let added = client
        .result(
            "project.add",
            json!({ "host_id": "local", "path": project["path"], "trusted": false }),
        )
        .await;
    let root = workspaces(&mut client, &added["id"]).await.remove(0);
    backend.file("rec-root", path(&root), path(&root));
    let listed = sessions(&mut client).await;
    assert_eq!(listed.len(), 1, "{listed:#?}");
    assert_eq!(listed[0]["title"], "in the root");
    assert_eq!(listed[0]["workspace_id"], root["id"]);
}

// verifies: FLOW-ARCHIVE-DELETE-SESSION-R05
#[tokio::test(flavor = "multi_thread")]
async fn a_found_session_uses_its_workspace_so_deleting_another_there_keeps_the_files() {
    let backend = Backend::default();
    let core = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let project = project(&mut client, core.dir.path()).await;
    let isolated = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    backend.record("rec-isolated", "found", path(&isolated), path(&isolated));
    let ersa = client
        .result(
            "session.create",
            json!({ "workspace_id": isolated["id"], "backend": fake::BACKEND }),
        )
        .await;
    client
        .result("session.stop", json!({ "id": ersa["id"] }))
        .await;
    assert_eq!(sessions(&mut client).await.len(), 2);

    // The found session is another session using the workspace: the delete
    // leaves the workspace and its files, and the found session stays.
    let changes = client
        .result("workspace.changes", json!({ "id": isolated["id"] }))
        .await;
    assert_eq!(changes["sessions"], 2);
    let deleted = client
        .result(
            "session.delete",
            json!({ "id": ersa["id"], "token": changes["token"] }),
        )
        .await;
    assert_eq!(deleted["workspace_deleted"], false);
    assert!(std::path::Path::new(path(&isolated)).exists());
    assert_eq!(workspaces(&mut client, &project["id"]).await.len(), 2);
    let listed = sessions(&mut client).await;
    assert_eq!(listed.len(), 1, "{listed:#?}");
    assert_eq!(listed[0]["title"], "found");
    assert!(backend.has_record("rec-isolated"));
}

// verifies: FLOW-RESUME-SESSION-R14
#[tokio::test(flavor = "multi_thread")]
async fn a_found_session_core_is_running_holds_back_removing_its_project() {
    let backend = Backend::default();
    let core = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let project = project(&mut client, core.dir.path()).await;
    let root = workspaces(&mut client, &project["id"]).await.remove(0);
    backend.record("rec-root", "found", path(&root), path(&root));
    let found = sessions(&mut client).await.remove(0);
    client
        .result("session.resume", json!({ "id": found["id"] }))
        .await;

    // No backend is left running in a project Ersa has forgotten.
    let refused = client
        .error("project.remove", json!({ "id": project["id"] }))
        .await;
    assert_eq!(support::kind(&refused), "invalid_state", "{refused}");
    assert_eq!(sessions(&mut client).await.len(), 1);
    client
        .result("session.stop", json!({ "id": found["id"] }))
        .await;
    client
        .result("project.remove", json!({ "id": project["id"] }))
        .await;
    assert!(sessions(&mut client).await.is_empty());
    assert!(backend.has_record("rec-root"));
}

// verifies: FLOW-RESUME-SESSION-R14
#[tokio::test(flavor = "multi_thread")]
async fn a_found_session_core_is_running_holds_back_removing_its_workspace() {
    let backend = Backend::default();
    let core = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let project = project(&mut client, core.dir.path()).await;
    let isolated = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    backend.record("rec-isolated", "found", path(&isolated), path(&isolated));
    let found = sessions(&mut client).await.remove(0);
    client
        .result("session.resume", json!({ "id": found["id"] }))
        .await;

    // No backend is left running in a removed folder.
    let refused = client
        .error("workspace.remove", json!({ "id": isolated["id"] }))
        .await;
    assert_eq!(support::kind(&refused), "invalid_state");
    client
        .result("session.stop", json!({ "id": found["id"] }))
        .await;
    client
        .result("workspace.remove", json!({ "id": isolated["id"] }))
        .await;
    assert!(sessions(&mut client).await.is_empty());
    assert!(backend.has_record("rec-isolated"));
}

/// Waits until the backend has been asked for folder listings `n` times.
async fn listings(backend: &Backend, n: usize) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while backend.stats.listings.load(Ordering::SeqCst) < n {
        assert!(tokio::time::Instant::now() < deadline, "no listing");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

// verifies: FLOW-RESUME-SESSION-R12
#[tokio::test(flavor = "multi_thread")]
async fn a_session_deleted_while_its_folder_is_listed_is_not_found_again() {
    let backend = Backend::default();
    let core = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let project = project(&mut client, core.dir.path()).await;
    let root = workspaces(&mut client, &project["id"]).await.remove(0);
    backend.record("rec-a", "found", path(&root), path(&root));
    let found = titled(&sessions(&mut client).await, "found").expect("listed")["id"].clone();

    // A list reads the folder, still holding the record, and is held there
    // while the user deletes the session, record and all.
    let held = backend.list_gate.lock().await;
    let mut lister = core.client().await;
    let listing = lister.request("session.list", json!({})).await;
    listings(&backend, 2).await;
    client
        .result("session.delete", json!({ "id": found }))
        .await;
    assert!(!backend.has_record("rec-a"));
    drop(held);

    let listed = lister.response(listing).await["result"].clone();
    assert_eq!(listed, json!([]), "{listed:#}");
    assert!(sessions(&mut client).await.is_empty());
}

// verifies: FLOW-RESUME-SESSION-R12
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_still_being_listed_is_not_listed_again() {
    let backend = Backend::default();
    let core = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let project = project(&mut client, core.dir.path()).await;
    let root = workspaces(&mut client, &project["id"]).await.remove(0);
    backend.record("rec-a", "found", path(&root), path(&root));

    // A file system that stopped answering: the list answers without the
    // folder once its time is up, and the next list leaves the folder to
    // the listing still under way rather than start another.
    let held = backend.list_gate.lock().await;
    assert!(sessions(&mut client).await.is_empty());
    assert_eq!(backend.stats.listings.load(Ordering::SeqCst), 1);
    assert!(sessions(&mut client).await.is_empty());
    assert_eq!(backend.stats.listings.load(Ordering::SeqCst), 1);

    // Once it answers, the folder is listed again.
    drop(held);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let listed = sessions(&mut client).await;
        if !listed.is_empty() {
            assert_eq!(listed[0]["title"], "found");
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "never listed");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(backend.stats.listings.load(Ordering::SeqCst) >= 2);
}

/// A Core over `adapter` whose git commands run behind `stall`.
async fn start_stalling(adapter: Arc<dyn Adapter>, stall: &Arc<Stall>) -> TestCore {
    let transport = Arc::new(StallingTransport {
        stall: Arc::clone(stall),
        inner: LocalTreeTransport::for_commands(),
    });
    start_with_adapters(
        tempdir(),
        move |config| config.command_transport = Some(CommandTransport(transport)),
        vec![adapter],
    )
    .await
}

// verifies: FLOW-RESUME-SESSION-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_found_session_written_to_while_its_workspace_is_removed_is_refused_and_not_started() {
    let backend = Backend::default();
    let stall = Stall::new();
    let core = start_stalling(backend.adapter(), &stall).await;
    let mut client = core.client().await;
    let mut remover = core.client().await;
    let project = project(&mut client, core.dir.path()).await;
    let isolated = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    backend.record("rec-isolated", "found", path(&isolated), path(&isolated));
    let found = sessions(&mut client).await.remove(0);

    // The removal holds the workspace and is at git, which does not answer
    // yet, as in a slow repository.
    stall.arm();
    let removal = remover
        .request("workspace.remove", json!({ "id": isolated["id"] }))
        .await;
    stall.reached().await;
    let starts = backend.stats.starts.load(Ordering::SeqCst);
    // Refused as busy, as while a delete holds the workspace.
    for (method, params) in [
        ("session.resume", json!({ "id": found["id"] })),
        (
            "session.send",
            json!({ "id": found["id"], "text": "hello" }),
        ),
    ] {
        let refused = client.error(method, params).await;
        assert_eq!(
            support::kind(&refused),
            "invalid_state",
            "{method}: {refused}"
        );
        assert_eq!(refused["data"]["detail"], "busy", "{method}: {refused}");
    }
    // No backend started in the folder about to go.
    assert_eq!(backend.stats.starts.load(Ordering::SeqCst), starts);

    stall.go();
    let removed = remover.response(removal).await;
    assert!(removed.get("error").is_none(), "{removed}");
    assert!(sessions(&mut client).await.is_empty());
    assert_eq!(backend.stats.starts.load(Ordering::SeqCst), starts);
}

// verifies: FLOW-RESUME-SESSION-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_git_refuses_leaves_its_found_sessions_as_they_were() {
    let backend = Backend::default();
    let core = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let project = project(&mut client, core.dir.path()).await;
    let isolated = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    backend.record("rec-isolated", "found", path(&isolated), path(&isolated));
    let found = sessions(&mut client).await.remove(0);
    // Work git would lose: the removal is refused without `force`.
    std::fs::write(
        std::path::Path::new(path(&isolated)).join("work.txt"),
        "work",
    )
    .unwrap();

    let refused = client
        .error("workspace.remove", json!({ "id": isolated["id"] }))
        .await;
    assert_eq!(support::kind(&refused), "vcs_error", "{refused}");

    // The hold is gone with the refusal: the session can be changed and
    // written to again.
    let pinned = client
        .result(
            "session.arrange",
            json!({ "id": found["id"], "pinned": true }),
        )
        .await;
    assert_eq!(pinned["pinned"], true);
    client
        .result("session.resume", json!({ "id": found["id"] }))
        .await;
}

// verifies: FLOW-RESUME-SESSION-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_project_removed_while_a_list_records_a_found_session_does_not_leave_it_listed() {
    let backend = Backend::default();
    let gate = Arc::new(ersa_core::Gate::default());
    let config_gate = Arc::clone(&gate);
    let core = start_with_adapters(
        tempdir(),
        move |config| config.found_gate = Some(config_gate),
        vec![backend.adapter()],
    )
    .await;
    let mut client = core.client().await;
    let mut remover = core.client().await;
    let project = project(&mut client, core.dir.path()).await;
    let root = workspaces(&mut client, &project["id"]).await.remove(0);
    backend.record("rec-a", "first", path(&root), path(&root));

    // A list has stored the session it found and not yet loaded it when the
    // project is removed. The removal takes the row and tells admission;
    // the list then finds admission without the session and does not load
    // it, where it would otherwise load a session whose project is gone.
    let list = client.request("session.list", json!({})).await;
    gate.reached().await;
    // The removal does not wait for the list: it is done, the row gone and
    // nothing loaded to let go of, before the list goes on.
    remover
        .result("project.remove", json!({ "id": project["id"] }))
        .await;
    gate.go();
    let listed = client.response(list).await;
    assert!(listed.get("error").is_none(), "{listed}");

    // The found session went with the project, its record left.
    let listed = sessions(&mut client).await;
    assert!(listed.is_empty(), "{listed:#?}");
    assert!(backend.has_record("rec-a"));
}

/// A delete that takes a workspace's files sets it to `error`
/// (PROTO-CORE-API-R57). A found session does not hold a removal back, so a
/// `workspace.remove` then claims the workspace; git refuses it, and it
/// leaves `error`, not the `ready` a workspace it took from `ready` gets
/// back.
// verifies: PROTO-CORE-API-R94
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_git_refuses_leaves_the_error_a_delete_set() {
    let backend = Backend::default();
    let core = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let project = project(&mut client, core.dir.path()).await;
    let isolated = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    backend.record("rec-isolated", "found", path(&isolated), path(&isolated));
    let found = sessions(&mut client).await.remove(0);
    // A locked worktree stops the delete at the files, its session kept,
    // and stops the removal after it.
    let repo = std::path::Path::new(project["path"].as_str().unwrap());
    support::git(repo, &["worktree", "lock", path(&isolated)]);
    let changes = client
        .result("workspace.changes", json!({ "id": isolated["id"] }))
        .await;
    let error = client
        .error(
            "session.delete",
            json!({ "id": found["id"], "token": changes["token"] }),
        )
        .await;
    assert_eq!(support::kind(&error), "vcs_error", "{error}");
    let listed = workspaces(&mut client, &project["id"]).await;
    assert_eq!(listed[1]["state"], "error", "{listed:#?}");

    let refused = client
        .error("workspace.remove", json!({ "id": isolated["id"] }))
        .await;
    assert_eq!(support::kind(&refused), "vcs_error", "{refused}");
    let listed = workspaces(&mut client, &project["id"]).await;
    assert_eq!(listed[1]["state"], "error", "{listed:#?}");
}
