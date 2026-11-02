//! One function per implemented Core API method. Each takes the parsed params
//! and returns the typed result or an [`ApiError`].

use std::path::{Component, Path};

use ersa_protocol::core_api::{
    BackendInfo, BackendListParams, Empty, HelloParams, HelloResult, Host, HostKind, HostStatus,
    IdParams, Project, ProjectAddParams, ProjectSetTrustParams, SUPPORTED_API_VERSIONS,
    SettingsSetParams, StatusResult, Vcs, Workspace, WorkspaceCreateParams, WorkspaceListParams,
    WorkspaceRemoveParams, WorkspaceState,
};
use ersa_protocol::events::PROJECTION_VERSION;
use serde_json::{Map, Value};

use super::{After, Answer, ApiError, ConnectionState};
use crate::persistence::{self, LOCAL_HOST_ID};
use crate::platform;
use crate::settings;
use crate::state::Core;
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

fn unreachable_host(host_id: &str) -> ApiError {
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
    // folder is inspected on this machine's file system, off the async
    // threads since file system calls block. Through a remote host's
    // transport these become commands run there.
    let raw = params.path.clone();
    let folder = tokio::task::spawn_blocking(move || inspect_folder(&raw))
        .await
        .map_err(|_| ApiError::internal("inspecting the folder panicked"))??;

    if let Some(existing) = core
        .store
        .project_at(params.host_id.clone(), folder.key.clone())
        .await?
    {
        return Err(
            ApiError::invalid_state(format!("{} is already a project", folder.path))
                .with_detail(existing.id),
        );
    }

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
    core.store
        .insert_project(project.clone(), folder.key)
        .await?;
    Ok(project)
}

/// A folder about to become a project.
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
    let raw = raw.trim();
    if !Path::new(raw).is_absolute() {
        return Err(ApiError::invalid_params(format!(
            "path must be absolute: {raw}"
        )));
    }
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
    let text = path.display().to_string();
    Ok(Folder {
        key: platform::path_key(&text),
        path: text,
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

pub(super) async fn project_remove(core: &Core, params: IdParams) -> Result<Project, ApiError> {
    let project = find_project(core, &params.id).await?;
    if !core.store.workspaces(project.id.clone()).await?.is_empty() {
        return Err(ApiError::invalid_state(format!(
            "project {} still has workspaces; remove them first",
            project.id
        )));
    }
    core.store.delete_project(project.id.clone()).await?;
    Ok(project)
}

async fn find_project(core: &Core, id: &str) -> Result<Project, ApiError> {
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

pub(super) async fn workspace_create(
    core: &Core,
    params: WorkspaceCreateParams,
) -> Result<Workspace, ApiError> {
    let project = find_project(core, &params.project_id).await?;
    if project.vcs != Vcs::Git {
        return Err(ApiError::invalid_state(format!(
            "project {} is not a git repository; only git worktrees are supported",
            project.id
        )));
    }
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
        path: path.clone(),
        vcs_ref: branch.clone(),
        state: WorkspaceState::Creating,
    };
    // Recorded as `creating` first, so a listing during a slow checkout shows it.
    core.store.insert_workspace(workspace.clone()).await?;

    let created = workspace::git_worktree_add(
        transport.as_ref(),
        &project.path,
        &path,
        &branch,
        params.base.as_deref(),
    )
    .await;
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
                    .set_workspace_state(id, WorkspaceState::Error)
                    .await?;
                return Err(e.into());
            }
        }
        core.store.delete_workspace(id).await?;
        return Err(e.into());
    }
    core.store
        .set_workspace_state(id, WorkspaceState::Ready)
        .await?;
    workspace.state = WorkspaceState::Ready;
    Ok(workspace)
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
    let project = find_project(core, &workspace.project_id).await?;
    let transport = core
        .transport_for(&project.host_id)
        .ok_or_else(|| unreachable_host(&project.host_id))?;

    // Claim the workspace, so a second removal (or, later, a session start)
    // cannot run against it at the same time. A workspace in `error` from a
    // failed removal may be retried.
    const REMOVABLE: &[WorkspaceState] = &[WorkspaceState::Ready, WorkspaceState::Error];
    let claimed = core
        .store
        .transition_workspace(workspace.id.clone(), REMOVABLE, WorkspaceState::Removing)
        .await?;
    if !claimed {
        return Err(ApiError::invalid_state(format!(
            "workspace {} is busy; try again when it is ready",
            workspace.id
        )));
    }

    // A workspace with sessions is refused, as a project with workspaces
    // is: removal never cascades, and no backend may be left running in a
    // deleted directory. Checked after the claim, because a session can
    // only be created in a `ready` workspace: one created before the claim
    // is seen here, and none can be created after it.
    if core
        .store
        .sessions_in_workspace(workspace.id.clone())
        .await?
        > 0
    {
        core.store
            .set_workspace_state(workspace.id.clone(), workspace.state)
            .await?;
        return Err(ApiError::invalid_state(format!(
            "workspace {} has sessions; stop and remove them first",
            workspace.id
        )));
    }

    let force = params.force.unwrap_or(false);
    let removed = workspace::git_worktree_remove(
        transport.as_ref(),
        &project.path,
        &workspace.path,
        force,
        // An `error` workspace may be an add that was killed mid-way;
        // its leftover `initializing` lock must not block removal.
        workspace.state == WorkspaceState::Error,
    )
    .await;
    if let Err(e) = removed {
        // A refusal (locked, local changes without `force`, or a folder git
        // does not list) leaves the worktree as it was, so it goes back to
        // the state it had; any other failure leaves it in an unknown state.
        let state = if e.is_refusal() {
            workspace.state
        } else {
            WorkspaceState::Error
        };
        core.store
            .set_workspace_state(workspace.id.clone(), state)
            .await?;
        return Err(e.into());
    }
    core.store.delete_workspace(workspace.id.clone()).await?;
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
