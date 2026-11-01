//! One function per implemented Core API method. Each takes the parsed params
//! and returns the typed result or an [`ApiError`].

use std::path::{Path, PathBuf};

use ersa_protocol::core_api::{
    BackendInfo, BackendListParams, Empty, HelloParams, HelloResult, Host, HostKind, HostStatus,
    IdParams, Project, ProjectAddParams, SUPPORTED_API_VERSIONS, SettingsSetParams, StatusResult,
    Vcs, Workspace, WorkspaceCreateParams, WorkspaceListParams, WorkspaceRemoveParams,
    WorkspaceState,
};
use ersa_protocol::events::PROJECTION_VERSION;
use serde_json::{Map, Value};

use super::{After, Answer, ApiError, ConnectionState};
use crate::persistence::{self, LOCAL_HOST_ID};
use crate::settings;
use crate::state::Core;
use crate::workspace;

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
    let (sessions_running, sessions_waiting) = core.store.session_counts().await?;
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
    // checks below use this machine's file system. Through a remote host's
    // transport they become commands run there.
    let path = Path::new(params.path.trim());
    if !path.is_absolute() {
        return Err(ApiError::invalid_params(format!(
            "path must be absolute: {}",
            params.path
        )));
    }
    if !path.is_dir() {
        return Err(ApiError::not_found(format!(
            "no directory at {}",
            params.path
        )));
    }
    // Rebuilding the path from its components drops a trailing separator and
    // uses the platform's separator throughout, so one directory always has
    // one spelling and is found by the duplicate check.
    let path: PathBuf = path.components().collect();
    let path_text = path.display().to_string();

    if let Some(existing) = core
        .store
        .project_at(params.host_id.clone(), path_text.clone())
        .await?
    {
        return Err(
            ApiError::invalid_state(format!("{path_text} is already a project"))
                .with_detail(existing.id),
        );
    }

    let project = Project {
        id: persistence::new_id("p"),
        host_id: params.host_id,
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path_text.clone()),
        path: path_text,
        vcs: detect_vcs(&path),
    };
    core.store.insert_project(project.clone()).await?;
    Ok(project)
}

/// A project is a git project when it has a `.git` entry: a directory in an
/// ordinary clone, a file in a worktree or submodule. Perforce detection
/// arrives with the Perforce workspace adapter (M4).
fn detect_vcs(path: &Path) -> Vcs {
    if path.join(".git").exists() {
        Vcs::Git
    } else {
        Vcs::None
    }
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
    let path = root.join(&project.name).join(&id).display().to_string();

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
        // Nothing was created, so nothing is left to remove later.
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

    // Stop, then remove. M0-T6: stop this workspace's sessions here, before
    // the files go, so no backend is left running in a deleted directory.

    let force = params.force.unwrap_or(false);
    let removed =
        workspace::git_worktree_remove(transport.as_ref(), &project.path, &workspace.path, force)
            .await;
    if let Err(e) = removed {
        // A refusal (local changes without `force`) leaves the worktree as it
        // was; any other failure leaves it in an unknown state.
        let state = match &e {
            workspace::VcsError::Failed { stderr, .. } if stderr.contains("--force") => {
                WorkspaceState::Ready
            }
            _ => WorkspaceState::Error,
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
