//! One function per implemented Core API method. Each takes the parsed params
//! and returns the typed result or an [`ApiError`].

use std::path::{Component, Path};
use std::sync::Arc;

use ersa_protocol::core_api::{
    BackendInfo, BackendListParams, Empty, HelloParams, HelloResult, Host, HostKind, HostStatus,
    IdParams, Project, ProjectAddParams, ProjectFindParams, ProjectSetTrustParams,
    SUPPORTED_API_VERSIONS, SettingsSetParams, StatusResult, Vcs, Workspace, WorkspaceChanges,
    WorkspaceCreateParams, WorkspaceKind, WorkspaceListParams, WorkspaceRemoveParams,
    WorkspaceState,
};
use ersa_protocol::events::PROJECTION_VERSION;
use serde_json::{Map, Value};

use super::{After, Answer, ApiError, ConnectionState, lock};
use crate::admission::{self, Command, Refusal, Replaced, Report};
use crate::persistence::{self, LOCAL_HOST_ID};
use crate::platform;
use crate::settings;
use crate::state::{Core, Inspection};
use crate::workspace::Worktrees as _;
use crate::workspace::{self, VcsError};

// -- Core ----------------------------------------------------------------

pub(super) fn core_hello(core: &Core, conn: &mut ConnectionState, params: HelloParams) -> Answer {
    if conn.api_version.is_some() {
        return Err((
            ApiError::invalid_state("core.hello was already received on this connection"),
            After::Continue,
        ));
    }
    // The highest version both sides speak.
    let shared = params
        .api_versions
        .iter()
        .copied()
        .filter(|v| SUPPORTED_API_VERSIONS.contains(v))
        .max();
    let Some(api_version) = shared else {
        // Name the versions Core supports, then close (PROTO-CORE-API-R07).
        let supported = list(SUPPORTED_API_VERSIONS);
        let error = ApiError::invalid_state(format!(
            "no shared api_version: this Core supports {supported}; the client offered {}",
            list(&params.api_versions)
        ))
        .with_detail(format!("supported api_versions: {supported}"));
        return Err((error, After::Close));
    };
    conn.api_version = Some(api_version);
    tracing::info!(client = %params.client, client_version = %params.client_version, api_version, "client attached");

    let result = HelloResult {
        core_version: env!("CARGO_PKG_VERSION").to_string(),
        api_version,
        event_projection_version: PROJECTION_VERSION,
        host: core.hello_host(),
        capabilities: Vec::new(),
    };
    let value = serde_json::to_value(result)
        .map_err(|e| (ApiError::internal(e.to_string()), After::Close))?;
    Ok((value, After::Continue))
}

fn list(versions: &[u32]) -> String {
    let items: Vec<String> = versions.iter().map(u32::to_string).collect();
    format!("[{}]", items.join(", "))
}

pub(super) async fn core_status(core: &Core, _: Empty) -> Result<StatusResult, ApiError> {
    let (sessions_running, sessions_waiting) = core.sessions.counts();
    Ok(StatusResult {
        sessions_running,
        sessions_waiting,
        uptime_s: core.uptime_s(),
        db_path: core.store.path().display().to_string(),
    })
}

// -- Hosts and backends --------------------------------------------------

pub(super) async fn host_list(core: &Core, _: Empty) -> Result<Vec<Host>, ApiError> {
    let mut hosts = Vec::new();
    for row in core.store.hosts().await? {
        let host = if row.id == LOCAL_HOST_ID {
            Host {
                id: row.id,
                name: row.name,
                kind: HostKind::Local,
                status: HostStatus::Connected,
                backends: core
                    .local_backends()
                    .await
                    .iter()
                    .map(|d| d.status())
                    .collect(),
            }
        } else {
            // Remote hosts (M4) report their real status once they exist.
            Host {
                id: row.id,
                name: row.name,
                kind: row.kind,
                status: HostStatus::Unknown,
                backends: Vec::new(),
            }
        };
        hosts.push(host);
    }
    Ok(hosts)
}

pub(super) async fn backend_list(
    core: &Core,
    params: BackendListParams,
) -> Result<Vec<BackendInfo>, ApiError> {
    if core.store.host(params.host_id.clone()).await?.is_none() {
        return Err(ApiError::not_found(format!("no host {}", params.host_id)));
    }
    if params.host_id != LOCAL_HOST_ID {
        return Err(unreachable_host(&params.host_id));
    }
    Ok(core
        .local_backends()
        .await
        .iter()
        .map(|d| d.info())
        .collect())
}

pub(super) fn unreachable_host(host_id: &str) -> ApiError {
    ApiError::new(
        ersa_protocol::core_api::ErrorKind::TransportError,
        format!("host {host_id} cannot be reached"),
    )
}

// -- Projects ------------------------------------------------------------

pub(super) async fn project_list(core: &Core, _: Empty) -> Result<Vec<Project>, ApiError> {
    Ok(core.store.projects().await?)
}

pub(super) async fn project_add(
    core: &Core,
    params: ProjectAddParams,
) -> Result<Project, ApiError> {
    if core.store.host(params.host_id.clone()).await?.is_none() {
        return Err(ApiError::not_found(format!("no host {}", params.host_id)));
    }
    if core.transport_for(&params.host_id).is_none() {
        return Err(unreachable_host(&params.host_id));
    }

    // Project paths are on the local host until remote hosts (M4), so the
    // folder is inspected on this machine's file system. Through a remote
    // host's transport this becomes commands run there.
    let raw = params.path.clone();
    let folder = inspect(core, &params.path, move || inspect_folder(&raw)).await?;

    let project = Project {
        id: persistence::new_id("p"),
        host_id: params.host_id,
        path: folder.path,
        name: folder.name,
        vcs: folder.vcs,
        // Only what the user answered: a project's own configuration loads
        // in its sessions only once they have said they trust it (ADR 0018).
        trusted: params.trusted,
    };
    // Its root workspace, the project's own folder, is made with it.
    // Admission decides whether the folder is already a project, and holds
    // this add behind another of the same folder still under way, so of two
    // only one adds it (PROTO-CORE-API-R20).
    let root = persistence::new_id("w");
    let adding = core
        .admission
        .command(Command::AddProject {
            project: project.id.clone(),
            root: root.clone(),
            folder: admission::Folder {
                host: project.host_id.clone(),
                key: folder.key.clone(),
            },
        })
        .await
        .map_err(|refusal| refused(refusal, &project.path))?;
    core.store
        .insert_project(project.clone(), root, folder.key)
        .await?;
    adding.end(Report::ProjectStored {
        project: project.id.clone(),
    });
    Ok(project)
}

/// The answer for a command admission refused, worded as before admission
/// decided it. `named` is what the request was about: the folder's path for
/// `project.add`, else the project's or workspace's id.
fn refused(refusal: Refusal, named: &str) -> ApiError {
    match refusal {
        Refusal::AlreadyProject { existing } => {
            ApiError::invalid_state(format!("{named} is already a project")).with_detail(existing)
        }
        Refusal::NoProject => ApiError::not_found(format!("no project {named}")),
        // The project's own folder is held, by another removal of the
        // project or by a delete of a session found there.
        Refusal::ProjectBusy { root } => {
            crate::sessions::busy(root.as_deref().unwrap_or(named)).into()
        }
        // A delete of a session found there holds the workspace.
        Refusal::WorkspaceHeld => crate::sessions::busy(named).into(),
        Refusal::ProjectHasSessions => ApiError::invalid_state(format!(
            "project {named} has sessions in its own folder; stop and remove them first"
        )),
        Refusal::ProjectHasWorkspaces => ApiError::invalid_state(format!(
            "project {named} still has workspaces; remove them first"
        )),
        Refusal::NoWorkspace => ApiError::not_found(format!("no workspace {named}")),
        Refusal::RootWorkspace => ApiError::invalid_state(format!(
            "workspace {named} is the project's own folder; it is removed only with the project"
        )),
        Refusal::WorkspaceBusy => ApiError::invalid_state(format!(
            "workspace {named} is busy; try again when it is ready"
        )),
        Refusal::WorkspaceHasSessions => ApiError::invalid_state(format!(
            "workspace {named} has sessions; stop and remove them first"
        )),
        // Answers to a session's admission, or an id used twice: Core's fault.
        Refusal::WorkspaceNotReady
        | Refusal::WorkspaceExists
        | Refusal::SessionExists
        | Refusal::NoSession
        | Refusal::Closing
        | Refusal::Running
        | Refusal::BeingArchived
        | Refusal::SessionDeleting
        | Refusal::Archived
        | Refusal::NotRunning
        | Refusal::SendInProgress
        | Refusal::NoSendToCancel
        | Refusal::BeingForked
        | Refusal::Forking
        | Refusal::NoSection
        | Refusal::GaveUp => ApiError::internal(format!("admission refused {named}: {refusal:?}")),
    }
}

/// The project on the host whose folder is `params.path`, compared as
/// `project.add` compares folders, or `None` (PROTO-CORE-API-R27).
///
/// A find of a folder whose inspection for an earlier find is still under
/// way, past that find's limit too, joins it rather than start another: a
/// user retrying a folder on a share that stopped answering holds one
/// thread for it, not one per try.
pub(super) async fn project_find(
    core: &Core,
    params: ProjectFindParams,
) -> Result<Option<Project>, ApiError> {
    if core.store.host(params.host_id.clone()).await?.is_none() {
        return Err(ApiError::not_found(format!("no host {}", params.host_id)));
    }
    let subject = (params.host_id.clone(), params.path.trim().to_string());
    // Joined, or put in the map for later finds to join. The inspection is
    // started only once the map is unlocked, since its end takes the lock.
    let (answer, to_start) = {
        let mut finding = lock(&core.finding);
        match finding.get(&subject) {
            Some(under_way) => (under_way.clone(), None),
            None => {
                let (done, answer) = tokio::sync::watch::channel(None);
                finding.insert(subject.clone(), answer.clone());
                (answer, Some(done))
            }
        }
    };
    if let Some(done) = to_start {
        // Only this inspection takes its entry out, once, when it ends,
        // however it ends: a failure to start it included.
        let finished = {
            let finding = Arc::clone(&core.finding);
            move || {
                lock(&finding).remove(&subject);
            }
        };
        // A folder that cannot be opened is no project: `project.add`
        // would refuse it, so it matches none, even one added there before
        // it went.
        let raw = params.path.clone();
        let inspection = move || {
            let raw = absolute(&raw)?;
            Ok(platform::canonicalize(Path::new(raw))
                .ok()
                .map(|p| platform::folder_key(&p)))
        };
        start_inspection(core, &params.path, inspection, done, finished)?;
    }
    let Some(key) = answer_of(core, &params.path, answer).await? else {
        return Ok(None);
    };
    Ok(core.store.project_at(params.host_id, key).await?)
}

/// Runs `inspection` of the folder the user named at `raw` and waits for
/// it, as [`start_inspection`] and [`answer_of`] say.
async fn inspect<T: Clone + Send + Sync + 'static>(
    core: &Core,
    raw: &str,
    inspection: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    let (done, answer) = tokio::sync::watch::channel(None);
    start_inspection(core, raw, inspection, done, || {})?;
    answer_of(core, raw, answer).await
}

/// Starts `inspection` of the folder the user named at `raw`, on a thread
/// of its own since file system calls block, to answer through `done`.
/// `finished` is called once, when the thread ends, or here when no thread
/// was started. A hung file system call cannot be called off, so its
/// thread is left to finish, and since nothing waits for its result any
/// more, nothing comes of it when it does.
///
/// The thread is a plain one, not one of Tokio's blocking pool: that pool
/// also runs every database call, so threads stuck on a dead share there
/// would in time leave none for the database, and the runtime waits for
/// them when Core exits. It holds a place among
/// [`crate::state::FOLDER_INSPECTIONS`] until the call returns; with none
/// free, so many folders are not answering that the answer is
/// `permission` at once.
fn start_inspection<T: Send + Sync + 'static>(
    core: &Core,
    raw: &str,
    inspection: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
    done: tokio::sync::watch::Sender<Option<Result<T, ApiError>>>,
    finished: impl FnOnce() + Send + 'static,
) -> Result<(), ApiError> {
    // Calls `finished` on every way out: dropped here on a refusal, or by
    // the thread as it ends.
    let finished = OnDrop(Some(finished));
    let place = Arc::clone(&core.folder_inspections)
        .try_acquire_owned()
        .map_err(|_| {
            ApiError::new(
                ersa_protocol::core_api::ErrorKind::Permission,
                format!(
                    "{} was not inspected: too many folders asked about are not answering; \
                     try again once they do",
                    raw.trim()
                ),
            )
        })?;
    let gate = core.config.folder_gate.clone();
    let runtime = tokio::runtime::Handle::current();
    std::thread::Builder::new()
        .name("folder inspection".into())
        .spawn(move || {
            // Dropped in reverse order: the place, then `finished`, so a
            // later request finds the inspection gone only once its place
            // is free.
            let _finished = finished;
            let _place = place;
            // In tests, the folder that does not answer.
            if let Some(gate) = gate {
                runtime.block_on(gate.stop());
            }
            // `send_replace`, since every request may have stopped waiting:
            // nothing to do then.
            done.send_replace(Some(inspection()));
        })
        .map_err(|e| ApiError::internal(format!("could not start the folder inspection: {e}")))?;
    Ok(())
}

/// Calls the function it holds when dropped, a panic's unwinding included.
struct OnDrop<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> Drop for OnDrop<F> {
    fn drop(&mut self) {
        if let Some(f) = self.0.take() {
            f();
        }
    }
}

/// Waits for `answer`, the inspection of the folder the user named at
/// `raw`, for [`crate::CoreConfig::folder_inspection_limit`] from now: a
/// folder on a share that stopped answering never answers. Past the limit
/// the answer is `permission` (PROTO-CORE-API-R93).
async fn answer_of<T: Clone>(
    core: &Core,
    raw: &str,
    mut answer: Inspection<T>,
) -> Result<T, ApiError> {
    let limit = core.config.folder_inspection_limit;
    match tokio::time::timeout(limit, answer.wait_for(Option::is_some)).await {
        Ok(Ok(answered)) => answered
            .clone()
            .unwrap_or_else(|| Err(ApiError::internal("the folder inspection had no answer"))),
        // The inspection ended without an answer: it panicked, or its
        // thread could not be started for the request it was started for.
        Ok(Err(_)) => Err(ApiError::internal(
            "the folder inspection ended without an answer",
        )),
        Err(_) => Err(ApiError::new(
            ersa_protocol::core_api::ErrorKind::Permission,
            format!(
                "{} did not answer in time; it may be on a drive or share that stopped answering",
                raw.trim()
            ),
        )),
    }
}

/// `raw` without surrounding white space, refused unless it is absolute.
fn absolute(raw: &str) -> Result<&str, ApiError> {
    let raw = raw.trim();
    if !Path::new(raw).is_absolute() {
        return Err(ApiError::invalid_params(format!(
            "path must be absolute: {raw}"
        )));
    }
    Ok(raw)
}

/// A folder about to become a project.
#[derive(Clone)]
struct Folder {
    /// Canonical: absolute, links and `..` resolved, the stored letter case.
    path: String,
    /// The uniqueness key: the path as the file system compares paths.
    key: String,
    /// The folder's own name, a single path component.
    name: String,
    vcs: Vcs,
}

/// Checks that `raw` names a folder that can be a project, and describes it.
/// Blocking.
fn inspect_folder(raw: &str) -> Result<Folder, ApiError> {
    let raw = absolute(raw)?;
    let path = platform::canonicalize(Path::new(raw)).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => ApiError::new(
            ersa_protocol::core_api::ErrorKind::Permission,
            format!("cannot open {raw}: {e}"),
        ),
        _ => ApiError::not_found(format!("no directory at {raw}")).with_detail(e.to_string()),
    })?;
    if !path.is_dir() {
        return Err(ApiError::not_found(format!("{raw} is not a directory")));
    }
    // The name becomes a folder name under `workspaces.root`, so it must be
    // one ordinary component: never a drive or share root, never `..`.
    let name = match path.components().next_back() {
        Some(Component::Normal(name)) => name.to_string_lossy().into_owned(),
        _ => {
            return Err(ApiError::invalid_params(format!(
                "{raw} is a drive or share root; a project must be a named folder"
            )));
        }
    };
    Ok(Folder {
        key: platform::folder_key(&path),
        path: path.display().to_string(),
        name,
        vcs: detect_vcs(&path),
    })
}

/// A project is a git project when it has a `.git` entry: a directory in an
/// ordinary clone, a file in a worktree or submodule. Perforce detection
/// arrives with the Perforce workspace adapter (M4). Blocking.
fn detect_vcs(path: &Path) -> Vcs {
    if path.join(".git").exists() {
        Vcs::Git
    } else {
        Vcs::None
    }
}

/// Records whether the user trusts a project. Sessions already running keep
/// the configuration they started with; the next start uses the new answer.
pub(super) async fn project_set_trust(
    core: &Core,
    params: ProjectSetTrustParams,
) -> Result<Project, ApiError> {
    core.store
        .set_project_trust(params.id.clone(), params.trusted)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("no project {}", params.id)))
}

/// Refused while the project has an isolated workspace or a session in its
/// root workspace; it never cascades. Its root workspace goes with it.
pub(super) async fn project_remove(core: &Core, params: IdParams) -> Result<Project, ApiError> {
    let project = find_project(core, &params.id).await?;
    let root = core
        .store
        .workspaces(project.id.clone())
        .await?
        .into_iter()
        .find(|workspace| workspace.kind == WorkspaceKind::Root)
        .map(|workspace| workspace.id);
    // Admission decides it against whatever else is asked of the project:
    // a workspace or session admitted before refuses it, and one asked for
    // after waits or is refused (ARCH-CORE-R14). A session found in its own
    // folder that Core runs refuses it too (FLOW-RESUME-SESSION-R14), and
    // one whose start is under way has the removal wait to see. Admitted,
    // the removal holds the folder: no backend starts there.
    let removing = core
        .admission
        .command(Command::RemoveProject {
            project: project.id.clone(),
        })
        .await
        .map_err(|refusal| refused(refusal, &project.id))?;
    let not_removed = Report::ProjectRemovalRefused {
        project: project.id.clone(),
    };
    // The rows of the sessions found in its folder go with it, their
    // records left: whatever is under way on one finishes first, and
    // nothing begins on them until Core has let go of them.
    let quiet = match &root {
        Some(root) => Some(core.sessions.quiet_found(root).await),
        None => None,
    };
    // One transaction: on a failure nothing of the project went.
    if let Err(e) = core.store.delete_project(project.id.clone()).await {
        removing.end(not_removed);
        return Err(e.into());
    }
    removing.end(Report::ProjectRemoved {
        project: project.id.clone(),
    });
    if let Some(quiet) = quiet {
        core.sessions.forget_found(quiet).await;
    }
    Ok(project)
}

pub(super) async fn find_project(core: &Core, id: &str) -> Result<Project, ApiError> {
    core.store
        .project(id.to_string())
        .await?
        .ok_or_else(|| ApiError::not_found(format!("no project {id}")))
}

// -- Workspaces ----------------------------------------------------------

pub(super) async fn workspace_list(
    core: &Core,
    params: WorkspaceListParams,
) -> Result<Vec<Workspace>, ApiError> {
    let project = find_project(core, &params.project_id).await?;
    Ok(core.store.workspaces(project.id).await?)
}

/// What deleting an isolated workspace would lose, for the delete question
/// (PROTO-CORE-API-R48, R46). Answers for a workspace in any state, a half-removed
/// one included, counting what is left of it.
pub(super) async fn workspace_changes(
    core: &Core,
    params: IdParams,
) -> Result<WorkspaceChanges, ApiError> {
    let workspace = core
        .store
        .workspace(params.id.clone())
        .await?
        .ok_or_else(|| ApiError::not_found(format!("no workspace {}", params.id)))?;
    Ok(changes_of(core, &workspace).await?.0)
}

/// `workspace.changes` for `workspace`, and the ids of the sessions it
/// counted: every one using the workspace but those marked deleting, which
/// are on their way out. `session.delete` computes it the same way, so the
/// token it compares is made as the user's was.
pub(super) async fn changes_of(
    core: &Core,
    workspace: &Workspace,
) -> Result<(WorkspaceChanges, Vec<String>), ApiError> {
    if workspace.kind == WorkspaceKind::Root {
        return Err(ApiError::invalid_state(format!(
            "workspace {} is the project's own folder, whose files are never deleted",
            workspace.id
        )));
    }
    let project = find_project(core, &workspace.project_id).await?;
    require_workspace_adapter(&project)?;
    let transport = core
        .transport_for(&project.host_id)
        .ok_or_else(|| unreachable_host(&project.host_id))?;

    // A session started in the backend's own tools that the list holds
    // counts too: it uses the workspace like any other (FLOW-ARCHIVE-DELETE-SESSION).
    let sessions = core
        .store
        .sessions_counted_in_workspace(workspace.id.clone())
        .await?;
    let found = workspace::git_worktree_changes(
        transport.as_ref(),
        &project.path,
        &workspace.path,
        &workspace.vcs_ref,
    )
    .await?;

    // The token is a digest of everything the counts were made from, plus
    // the sessions, so it changes when any of them does. `session.delete`
    // recomputes it and refuses when it differs from the one the user saw.
    let mut digest = workspace::Digest::default();
    for line in &found.evidence {
        digest.add(line);
    }
    for id in &sessions {
        digest.add(&format!("session {id}"));
    }
    let changes = WorkspaceChanges {
        uncommitted: found.uncommitted,
        ignored: found.ignored,
        unshared: found.unshared,
        stranded: found.stranded,
        sessions: sessions.len() as u64,
        unrecorded: found.unrecorded,
        token: digest.finish(),
    };
    Ok((changes, sessions))
}

/// Refuses a project whose version control has no workspace adapter. Git is
/// the only one so far; a session of any other project runs in its root
/// workspace.
fn require_workspace_adapter(project: &Project) -> Result<(), ApiError> {
    if project.vcs == Vcs::Git {
        return Ok(());
    }
    Err(ApiError::invalid_state(format!(
        "project {} has no workspace adapter for its version control; \
         only git projects get isolated workspaces, so run the session in the project's own folder",
        project.id
    )))
}

pub(super) async fn workspace_create(
    core: &Core,
    params: WorkspaceCreateParams,
) -> Result<Workspace, ApiError> {
    let project = find_project(core, &params.project_id).await?;
    require_workspace_adapter(&project)?;
    for (field, value) in [("name", &params.name), ("base", &params.base)] {
        if let Some(value) = value
            && (value.is_empty() || value.starts_with('-'))
        {
            return Err(ApiError::invalid_params(format!(
                "{field} must be non-empty and must not start with '-'"
            )));
        }
    }
    let transport = core
        .transport_for(&project.host_id)
        .ok_or_else(|| unreachable_host(&project.host_id))?;
    let base = resolve_base(transport.as_ref(), &project.path, params.base.as_deref()).await?;

    let id = persistence::new_id("w");
    let branch = params.name.unwrap_or_else(|| format!("ersa/{id}"));
    // `<root>/<project name>/<id>`: outside the project tree, grouped by
    // project so the folder is readable, unique by id.
    let root = settings::workspaces_root(&core.store.settings().await?, &core.config.data_dir);
    // Canonical, so the path Core stores is the one git prints back: git
    // resolves links and junctions, and a root reached through one would
    // otherwise never match. Off the async threads; file system calls block.
    let root = tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&root)?;
        platform::canonicalize(&root)
    })
    .await
    .map_err(|_| ApiError::internal("preparing the workspaces root panicked"))?
    .map_err(|e| {
        ApiError::new(
            ersa_protocol::core_api::ErrorKind::Permission,
            format!("cannot use the workspaces root: {e}"),
        )
    })?;
    let path = workspace_path(&root, &project.name, &id)?;

    let mut workspace = Workspace {
        id: id.clone(),
        project_id: project.id.clone(),
        kind: WorkspaceKind::Isolated,
        path: path.clone(),
        vcs_ref: branch.clone(),
        state: WorkspaceState::Creating,
    };
    // Admitted before its row exists: a removal of the project decided
    // before this waits for nothing of it, and one asked for after sees it
    // (ARCH-CORE-R14). Each way out below stores what happened, then says
    // so; one that says nothing leaves admission holding it as `error`.
    let creating = core
        .admission
        .command(Command::CreateWorkspace {
            project: project.id.clone(),
            workspace: id.clone(),
        })
        .await
        .map_err(|refusal| refused(refusal, &project.id))?;
    let failed = |kept| Report::WorkspaceCreateFailed {
        workspace: id.clone(),
        kept,
    };
    // Recorded as `creating` first, so a listing during a slow checkout shows it.
    if let Err(e) = core.store.insert_workspace(workspace.clone()).await {
        creating.end(failed(false));
        return Err(e.into());
    }

    // The repository's turn, kept through the add and any clean-up of it.
    let turns = RepositoryTurns {
        core,
        project: &project.id,
    };
    let turn = match turns.turn().await {
        Ok(turn) => turn,
        Err(e) => {
            core.store.delete_workspace(id.clone()).await?;
            creating.end(failed(false));
            return Err(e.into());
        }
    };
    let created =
        workspace::git_worktree_add(transport.as_ref(), &project.path, &path, &branch, &base).await;
    if let Err(e) = created {
        // When git failed, it cleaned up after itself. When it timed out or
        // its output was lost, a half-made worktree may be on disk: discard
        // it, and if even that fails keep the row as `error`, so the user
        // sees it and can remove it later.
        if let VcsError::Incomplete(_) = e {
            let discarded =
                workspace::git_worktree_discard(transport.as_ref(), &project.path, &path).await;
            if let Err(discard_error) = discarded {
                tracing::warn!(workspace = %id, error = %discard_error, "could not clean up a failed worktree");
                core.store
                    .set_workspace_state(id.clone(), WorkspaceState::Error)
                    .await?;
                creating.end(failed(true));
                return Err(e.into());
            }
        }
        drop(turn);
        core.store.delete_workspace(id.clone()).await?;
        creating.end(failed(false));
        return Err(e.into());
    }
    drop(turn);
    core.store
        .set_workspace_state(id.clone(), WorkspaceState::Ready)
        .await?;
    creating.end(Report::WorkspaceReady { workspace: id });
    workspace.state = WorkspaceState::Ready;
    Ok(workspace)
}

/// The turns of a project's repository to have its worktrees changed, asked
/// of admission: a change waits while another is under way, at most
/// `worktree_turn_wait` ([`workspace::WORKTREE_TURN_WAIT`]).
pub(super) struct RepositoryTurns<'a> {
    pub core: &'a Core,
    pub project: &'a str,
}

#[ersa_adapter::async_trait]
impl workspace::Worktrees for RepositoryTurns<'_> {
    async fn turn(&self) -> Result<Box<dyn Send>, VcsError> {
        let wait = self.core.config.worktree_turn_wait;
        match self.core.admission.worktrees(self.project, wait).await {
            Ok(under_way) => Ok(Box::new(under_way)),
            Err(Refusal::GaveUp) => Err(VcsError::WorktreesBusy),
            // Only a project that is gone; nothing that changes worktrees
            // runs then, since a project with workspaces is not removed.
            Err(refusal) => Err(VcsError::Failed {
                message: format!("the project's worktrees cannot be changed: {refusal:?}"),
                stderr: String::new(),
            }),
        }
    }
}

/// The commit a new workspace starts from (PROTO-CORE-API-R19): `base` when
/// given, refused with `invalid_params` when git cannot resolve it; without
/// one, the repository's default branch, else the checked-out commit. Resolved to a commit id once, here, so the worktree starts from
/// exactly what was checked, and git never reads `base` as anything else.
async fn resolve_base(
    transport: &dyn ersa_adapter::Transport,
    repo: &str,
    base: Option<&str>,
) -> Result<String, ApiError> {
    if let Some(base) = base {
        return workspace::git_resolve_commit(transport, repo, base)
            .await?
            .ok_or_else(|| {
                ApiError::invalid_params(format!("git cannot resolve base {base} to a commit"))
            });
    }
    // Finding the default branch and resolving it share one limit.
    let (revision, commit) = workspace::git_default_commit(transport, repo).await?;
    // Nothing the caller gave can be wrong here, so a failure is git's: a
    // repository with no commit yet, or a branch deleted in between.
    commit.ok_or_else(|| {
        ApiError::new(
            ersa_protocol::core_api::ErrorKind::VcsError,
            format!("the repository has no commit to start from at {revision}"),
        )
    })
}

pub(super) async fn workspace_remove(
    core: &Core,
    params: WorkspaceRemoveParams,
) -> Result<Workspace, ApiError> {
    let mut workspace = core
        .store
        .workspace(params.id.clone())
        .await?
        .ok_or_else(|| ApiError::not_found(format!("no workspace {}", params.id)))?;
    // The root workspace is the project's own folder: it goes only with the
    // project, and nothing of it is ever deleted.
    if workspace.kind == WorkspaceKind::Root {
        return Err(ApiError::invalid_state(format!(
            "workspace {} is the project's own folder; it is removed only with the project",
            workspace.id
        )));
    }
    let project = find_project(core, &workspace.project_id).await?;
    let transport = core
        .transport_for(&project.host_id)
        .ok_or_else(|| unreachable_host(&project.host_id))?;

    // Admission decides the removal in one step: refused while the
    // workspace is being made or removed, or has a session Ersa started or
    // one Core runs (FLOW-RESUME-SESSION-R14), as a project with workspaces
    // is, since removal never cascades and no backend may be left running
    // in a deleted directory.
    // Once admitted it holds the workspace: no session is created in it,
    // none of its found sessions is sent to or resumed, no delete marks it
    // and no second removal starts (ARCH-CORE-R14). So from here to the
    // end, only this removal writes the workspace's state. A workspace in
    // `error` from a failed removal may be retried.
    let id = workspace.id.clone();
    let removing = core
        .admission
        .command(Command::RemoveWorkspace {
            workspace: id.clone(),
        })
        .await
        .map_err(|refusal| refused(refusal, &id))?;
    let not_removed = Report::WorkspaceRemovalRefused {
        workspace: id.clone(),
    };
    // What a refusal puts back: the state the removal took the workspace
    // from, `error` if a delete had marked it (PROTO-CORE-API-R94).
    let replaced = match core.admission.replaced(&id) {
        Some(Replaced::Error) => WorkspaceState::Error,
        Some(Replaced::Ready) | None => WorkspaceState::Ready,
    };
    let stored = core
        .store
        .set_workspace_state(id.clone(), WorkspaceState::Removing)
        .await;
    if let Err(e) = stored {
        // Nothing was written: the workspace is as it was.
        removing.end(not_removed);
        return Err(e.into());
    }

    // As every removal (PROTO-CORE-API-R122): the folder, git's record and
    // the branch, unless the branch holds work of its own or another
    // worktree has it checked out. Whether it was kept is not reported.
    let checks = workspace::RemovalChecks {
        force: params.force.unwrap_or(false),
        // An `error` workspace may be an add that was killed mid-way; its
        // leftover `initializing` lock must not block removal.
        after_failed_add: replaced == WorkspaceState::Error,
    };
    let removed = workspace::git_worktree_delete(
        transport.as_ref(),
        &project.path,
        &workspace.path,
        &workspace.vcs_ref,
        checks,
        // The removal's operation ends with what happened, reported below,
        // so a try left stuck holds nothing of it.
        workspace::FolderTries::from_config(&core.config, Arc::new(())),
        &RepositoryTurns {
            core,
            project: &project.id,
        },
    )
    .await;
    if let Err(e) = removed {
        // A refusal (locked, local changes or a folder git does not list
        // without `force`, or no turn at the worktrees in time) leaves the
        // worktree as it was, so it goes back to the state it had; any other
        // failure leaves it in an unknown state.
        // Stored, then said: with the report the hold goes, and the found
        // sessions are theirs again.
        if e.is_refusal() {
            core.store.set_workspace_state(id, replaced).await?;
            removing.end(not_removed);
        } else {
            core.store
                .set_workspace_state(id.clone(), WorkspaceState::Error)
                .await?;
            removing.end(Report::WorkspaceRemovalFailed { workspace: id });
        }
        return Err(e.into());
    }
    // The rows of the sessions found in its folder go with it, their
    // records left. A failure leaves the row `removing` for the next start
    // to mark `error`, which is how admission then holds it.
    // Whatever is under way on one of those sessions finishes first, and
    // nothing begins on them until Core has let go of them.
    let quiet = core.sessions.quiet_found(&id).await;
    core.store.delete_workspace(id.clone()).await?;
    removing.end(Report::WorkspaceRemoved {
        workspace: id.clone(),
    });
    core.sessions.forget_found(quiet).await;
    workspace.state = WorkspaceState::Removing;
    Ok(workspace)
}

/// `<root>/<project name>/<id>`: outside the project tree, grouped by
/// project so the folder is readable, unique by id. Checked to lie under
/// `root`, since removal deletes whatever folder this names.
fn workspace_path(root: &Path, project_name: &str, id: &str) -> Result<String, ApiError> {
    let path = root.join(project_name).join(id);
    let relative = path.strip_prefix(root).ok();
    let inside = relative.is_some_and(|rel| {
        rel.components().count() == 2 && rel.components().all(|c| matches!(c, Component::Normal(_)))
    });
    if !inside {
        return Err(ApiError::internal(format!(
            "workspace path {} would not be under {}",
            path.display(),
            root.display()
        )));
    }
    Ok(path.display().to_string())
}

// -- Settings ------------------------------------------------------------

pub(super) async fn settings_get(core: &Core, _: Empty) -> Result<Map<String, Value>, ApiError> {
    Ok(core.store.settings().await?)
}

pub(super) async fn settings_set(
    core: &Core,
    params: SettingsSetParams,
) -> Result<Map<String, Value>, ApiError> {
    Ok(core.store.patch_settings(params.patch).await?)
}
