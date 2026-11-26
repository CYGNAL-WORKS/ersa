//! The Clean up list (FLOW-CLEAN-UP, The list): what Core can remove, and
//! what removing each item would lose. Both methods only read; removing is
//! `cleanup.remove`'s.
//!
//! An item is one of four kinds (FLOW-CLEAN-UP-R11): the folder of a
//! workspace whose sessions are all archived or all inactive, a leftover
//! workspace as the sweep judges one, an `ersa/…` branch nothing uses, and
//! a folder under the worktrees root nothing names. Its `id` says which
//! thing it is, never which kind, so `cleanup.inspect` judges it again as
//! it is now: a workspace whose sessions are all deleted turns from a
//! `workspace` into a `leftover` and keeps its id.
//!
//! Whatever Core can't tell is said, never taken as nothing: a project
//! whose git calls fail is named in `unread`, and an inspect that can't
//! count what an item holds fails with why.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ersa_protocol::core_api::{
    CleanupInspection, CleanupItem, CleanupKind, CleanupListParams, CleanupListResult,
    CleanupUnread, ErrorKind, IdParams, Project, Session, Vcs, Workspace, WorkspaceKind,
    WorkspaceState,
};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::ApiError;
use super::handlers::{self, find_project};
use super::leftover::LEFTOVER_AGE;
use crate::persistence::SessionRow;
use crate::platform;
use crate::settings;
use crate::state::Core;
use crate::workspace::{self, Checkout, Survey};

/// The `inactive_days` `cleanup.list` takes (PROTO-CORE-API, Clean up).
const INACTIVE_DAYS: [u32; 3] = [30, 60, 90];

/// How long an inspect's counts may take, all of them together, as
/// `workspace.changes`'s do (PROTO-CORE-API, Clean up).
const COUNTS_LIMIT: Duration = Duration::from_secs(60);

/// The start of an item's id, for each kind of thing it names.
const WORKSPACE_ID: &str = "w:";
const BRANCH_ID: &str = "b:";
const FOLDER_ID: &str = "f:";

/// `cleanup.list` (FLOW-CLEAN-UP-R11).
pub(super) async fn list(
    core: &Core,
    params: CleanupListParams,
) -> Result<CleanupListResult, ApiError> {
    if !INACTIVE_DAYS.contains(&params.inactive_days) {
        return Err(ApiError::invalid_params(format!(
            "inactive_days must be 30, 60 or 90, not {}",
            params.inactive_days
        )));
    }
    let world = World::read(core).await?;
    let mut items = Vec::new();
    let mut unread = Vec::new();

    // Each git project's repository, read once. A project that can't be
    // read gives no branch or folder item, and no leftover whose standing
    // needs its worktrees.
    let mut surveys: BTreeMap<String, Survey> = BTreeMap::new();
    for project in world.git_projects() {
        match survey(core, project).await {
            Ok(survey) => {
                surveys.insert(project.id.clone(), survey);
            }
            Err(e) => unread.push(CleanupUnread {
                project_id: project.id.clone(),
                message: e.0.message,
            }),
        }
    }

    for workspace in &world.workspaces {
        let survey = surveys.get(&workspace.project_id);
        let judged = judge_workspace(core, &world, workspace, Some(params.inactive_days), survey);
        if let Some(item) = judged.await? {
            items.push(item);
        }
    }
    items.extend(branch_items(core, &world, &surveys).await?);
    let (folders, unreadable) = folder_items(core, &world, &surveys).await?;
    items.extend(folders);
    unread.extend(unreadable);
    Ok(CleanupListResult { items, unread })
}

/// `cleanup.inspect` (FLOW-CLEAN-UP-R12, R26): the item as it is now, with
/// what removing it would lose. Holds one of the places R116 counts from
/// its start to its end, and with none free answers `vcs_error` at once.
pub(super) async fn inspect(core: &Core, params: IdParams) -> Result<CleanupInspection, ApiError> {
    workspace::in_one_place(inspect_held(core, &params.id)).await?
}

async fn inspect_held(core: &Core, id: &str) -> Result<CleanupInspection, ApiError> {
    let gone = || ApiError::not_found(format!("no Clean up item {id}"));
    let world = World::read(core).await?;
    if let Some(workspace_id) = id.strip_prefix(WORKSPACE_ID) {
        let Some(workspace) = world.workspaces.iter().find(|w| w.id == workspace_id) else {
            return Err(gone());
        };
        let project = find_project(core, &workspace.project_id).await?;
        // Only a put-away workspace's standing needs the worktrees.
        let survey = if workspace.state == WorkspaceState::PutAway {
            Some(survey(core, &project).await?)
        } else {
            None
        };
        let judged = judge_workspace(core, &world, workspace, None, survey.as_ref()).await?;
        let item = judged.ok_or_else(gone)?;
        return inspect_workspace(core, workspace, item).await;
    }
    if let Some(rest) = id.strip_prefix(BRANCH_ID) {
        let (project_id, branch) = rest.split_once(':').ok_or_else(gone)?;
        let project = world.project(project_id).ok_or_else(gone)?;
        let survey = survey(core, project).await?;
        let named = named_branches(core, &world).await?;
        let unused = survey.ersa_branches.iter().any(|b| b == branch)
            && !named.contains(branch)
            && !checked_out(&survey).contains(branch);
        if !unused {
            return Err(gone());
        }
        let transport = transport(core, project)?;
        let loss = workspace::git_branch_loss(transport.as_ref(), &project.path, branch).await?;
        let mut digest = workspace::Digest::default();
        digest.add(&format!("tip {}", loss.tip.as_deref().unwrap_or("none")));
        digest.add(&format!("merged {}", loss.merged));
        for commit in &loss.unshared {
            digest.add(&format!("unshared {commit}"));
        }
        let item = branch_item(project_id, branch);
        return Ok(CleanupInspection {
            unshared: Some(loss.unshared.len() as u64),
            merged: Some(loss.merged),
            ..CleanupInspection::of(item, digest.finish())
        });
    }
    if let Some(rest) = id.strip_prefix(FOLDER_ID) {
        let (project_id, name) = rest.split_once(':').ok_or_else(gone)?;
        let project = world.project(project_id).ok_or_else(gone)?;
        let root = worktrees_root(core).await?.ok_or_else(gone)?;
        let sharing = world.sharing_folder(&root, &project.name);
        let mut surveys = Vec::new();
        for project in sharing.iter().filter(|p| p.vcs == Vcs::Git) {
            surveys.push(survey(core, project).await?);
        }
        let folder = root.join(&project.name);
        let names = workspace::real_folders_in(&folder).await?;
        if !names.iter().any(|n| n == name) {
            return Err(gone());
        }
        let path = folder.join(name).display().to_string();
        let named = world.workspace_keys().await?;
        if !unidentified(&path, &named, &surveys).await? {
            return Err(gone());
        }
        let measure = measure(core, &path).await?;
        let mut digest = workspace::Digest::default();
        for line in &measure.evidence {
            digest.add(line);
        }
        // Not walked to the end, what is in it can't all go into the token:
        // one made new each time never matches, so no removal goes ahead on
        // what the user saw of it.
        if measure.size_bytes.is_none() {
            digest.add(&uuid::Uuid::new_v4().to_string());
        }
        let item = folder_item(project_id, name, path);
        return Ok(CleanupInspection {
            size_bytes: measure.size_bytes,
            ..CleanupInspection::of(item, digest.finish())
        });
    }
    Err(gone())
}

/// What removing a `workspace` or `leftover` item would lose: what
/// `workspace.changes` counts, whether its folder holds a repository of its
/// own, and what the folder takes.
async fn inspect_workspace(
    core: &Core,
    workspace: &Workspace,
    item: CleanupItem,
) -> Result<CleanupInspection, ApiError> {
    let put_away = workspace.state == WorkspaceState::PutAway;
    let counted = tokio::time::timeout(COUNTS_LIMIT, async {
        let (changes, _) = handlers::changes_of(core, workspace).await?;
        let holds = holds_repository(core, workspace, put_away, changes.unrecorded).await?;
        Ok::<_, ApiError>((changes, holds))
    })
    .await;
    let (changes, holds) = match counted {
        Ok(counted) => counted?,
        Err(_) => {
            return Err(ApiError::new(
                ErrorKind::VcsError,
                format!(
                    "counting what the workspace holds did not finish within {} seconds",
                    COUNTS_LIMIT.as_secs()
                ),
            ));
        }
    };
    // A put-away one has no folder to measure.
    let size_bytes = if put_away {
        None
    } else {
        measure(core, &workspace.path).await?.size_bytes
    };
    // `workspace.changes`'s token covers everything it counts; what else
    // removing the item loses is added to it.
    let mut digest = workspace::Digest::default();
    digest.add(&format!("kind {:?}", item.kind));
    digest.add(&changes.token);
    digest.add(&format!("holds_repository {holds}"));
    Ok(CleanupInspection {
        uncommitted: Some(changes.uncommitted),
        ignored: Some(changes.ignored),
        unshared: Some(changes.unshared),
        stranded: Some(changes.stranded),
        unrecorded: Some(changes.unrecorded),
        holds_repository: Some(holds),
        size_bytes,
        ..CleanupInspection::of(item, digest.finish())
    })
}

/// Whether the workspace's folder holds a repository of its own
/// (PROTO-CORE-API-R147), as the sweep's check finds it. A put-away
/// workspace, with no folder and no record, has nowhere to hold one. With
/// git's record gone the check can't be made: a folder still there then
/// counts as holding one, since Core can't tell that it does not.
async fn holds_repository(
    core: &Core,
    workspace: &Workspace,
    put_away: bool,
    unrecorded: bool,
) -> Result<bool, ApiError> {
    if put_away {
        return Ok(false);
    }
    let project = find_project(core, &workspace.project_id).await?;
    let transport = transport(core, &project)?;
    let (repo, path) = (project.path.as_str(), workspace.path.as_str());
    let holds = if unrecorded {
        workspace::git_holds_repository_unrecorded(transport.as_ref(), repo, path).await?
    } else {
        workspace::git_holds_repository(transport.as_ref(), repo, path).await?
    };
    Ok(holds)
}

/// Measures the folder at `path` within Core's limit for it.
async fn measure(core: &Core, path: &str) -> Result<workspace::Measure, ApiError> {
    let config = &core.config;
    let measure =
        workspace::measure_folder(path, config.measure_limit, config.measure_gate.clone()).await?;
    Ok(measure)
}

// -- Which things are items (FLOW-CLEAN-UP-R11) ----------------------------

/// What Core holds that the list judges by, read once per request.
struct World {
    projects: Vec<Project>,
    /// Every project's workspaces.
    workspaces: Vec<Workspace>,
    /// Every stored session, dormant ones and those found in the backend's
    /// own tools included.
    rows: Vec<SessionRow>,
    /// The sessions Core has loaded, as `session.list` gives them.
    sessions: Vec<Session>,
}

impl World {
    async fn read(core: &Core) -> Result<World, ApiError> {
        // A session started in the backend's own tools counts as any other
        // (FLOW-CLEAN-UP, Variations): found first, as the sweep finds them.
        core.sessions
            .find_elsewhere(|host| core.session_transport_for(host))
            .await?;
        let projects = core.store.projects().await?;
        let mut workspaces = Vec::new();
        for project in &projects {
            workspaces.extend(core.store.workspaces(project.id.clone()).await?);
        }
        Ok(World {
            projects,
            workspaces,
            rows: core.store.sessions().await?,
            sessions: core.sessions.list(),
        })
    }

    fn project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    /// The git projects, by id, so an item listed under one of several
    /// is always listed under the same one.
    fn git_projects(&self) -> Vec<&Project> {
        let mut projects: Vec<&Project> =
            self.projects.iter().filter(|p| p.vcs == Vcs::Git).collect();
        projects.sort_by(|a, b| a.id.cmp(&b.id));
        projects
    }

    /// The projects whose workspaces go in `<root>/<name>`, by id: projects
    /// of one name share it, compared as the file system compares names.
    fn sharing_folder(&self, root: &Path, name: &str) -> Vec<&Project> {
        let key = |name: &str| platform::path_key(&root.join(name).display().to_string());
        let mut sharing: Vec<&Project> = self
            .projects
            .iter()
            .filter(|p| key(&p.name) == key(name))
            .collect();
        sharing.sort_by(|a, b| a.id.cmp(&b.id));
        sharing
    }

    /// The key of every workspace's path, as two spellings of one folder
    /// compare equal.
    async fn workspace_keys(&self) -> Result<HashSet<String>, ApiError> {
        let paths: Vec<String> = self.workspaces.iter().map(|w| w.path.clone()).collect();
        Ok(workspace::location_keys(paths).await?.into_iter().collect())
    }
}

/// `workspace` as a `workspace` or `leftover` item, if it is one now.
/// With `inactive_days`, as `cleanup.list` judges a workspace: its sessions
/// all archived, or all inactive that long and none pinned, unseen or
/// subscribed to. Without, as inspect judges one: by what it is now, never
/// by how long it has gone unused. `survey` is its project's, needed only
/// for a put-away one.
async fn judge_workspace(
    core: &Core,
    world: &World,
    workspace: &Workspace,
    inactive_days: Option<u32>,
    survey: Option<&Survey>,
) -> Result<Option<CleanupItem>, ApiError> {
    // A root workspace is the user's own folder.
    if workspace.kind != WorkspaceKind::Isolated {
        return Ok(None);
    }
    // Anything under way on a session there, or about to be: one arriving,
    // starting, running, sent to, forking, removed or marked deleting.
    if core.admission.session_in_use(&workspace.id) {
        return Ok(None);
    }
    let there: Vec<&SessionRow> = world
        .rows
        .iter()
        .filter(|row| row.workspace_id == workspace.id)
        .collect();
    let branch = Some(workspace.vcs_ref.clone()).filter(|b| !b.is_empty());
    if there.is_empty() {
        return Ok(is_leftover(core, workspace, survey)
            .await?
            .then(|| CleanupItem {
                workspace_id: Some(workspace.id.clone()),
                path: Some(workspace.path.clone()),
                branch,
                ..item(
                    workspace_id(&workspace.id),
                    CleanupKind::Leftover,
                    &workspace.project_id,
                )
            }));
    }
    if workspace.state != WorkspaceState::Ready {
        return Ok(None);
    }
    // One marked deleting admission counts as in use, above. A dormant
    // one, whose backend has no adapter in this build, can't be asked
    // whether another process has it open, below, so it keeps the
    // workspace off the list too.
    let sessions: Vec<Option<&Session>> = there
        .iter()
        .map(|row| world.sessions.iter().find(|s| s.id == row.id))
        .collect();
    if let Some(days) = inactive_days {
        let archived = there.iter().all(|row| row.archived);
        let now = core.sweep_now();
        let inactive = |session: &Option<&Session>| {
            session.is_some_and(|s| {
                let at = s.last_activity_at.as_deref().unwrap_or(&s.created_at);
                OffsetDateTime::parse(at, &Rfc3339)
                    .is_ok_and(|at| now - at >= time::Duration::days(i64::from(days)))
            })
        };
        let unused = sessions.iter().all(inactive)
            && there
                .iter()
                .all(|row| !row.pinned && !row.unseen && !core.sessions.is_subscribed(&row.id));
        if !archived && !unused {
            return Ok(None);
        }
    }
    // Last, since it asks the operating system about each session. One
    // Core can't ask, a dormant one among them, may be open.
    for row in &there {
        let Some(transport) = core.session_transport_for(&row.host_id) else {
            return Ok(None);
        };
        match core.sessions.open_elsewhere(&row.id, transport).await {
            Ok(ersa_adapter::OpenElsewhere::No) => {}
            Ok(_) | Err(_) => return Ok(None),
        }
    }
    let mut session_ids: Vec<String> = there.iter().map(|row| row.id.clone()).collect();
    session_ids.sort();
    // RFC 3339 times in UTC sort as text.
    let last_activity_at = sessions
        .iter()
        .flatten()
        .filter_map(|s| s.last_activity_at.clone())
        .max();
    Ok(Some(CleanupItem {
        workspace_id: Some(workspace.id.clone()),
        path: Some(workspace.path.clone()),
        branch,
        session_ids: Some(session_ids),
        last_activity_at,
        ..item(
            workspace_id(&workspace.id),
            CleanupKind::Workspace,
            &workspace.project_id,
        )
    }))
}

/// Whether `workspace`, which no session refers to, is a leftover
/// (FLOW-CLEAN-UP-R23), as the sweep judges one: made at least an hour ago
/// and not being made or removed; a put-away one only when nothing of its
/// folder or of git's record of it is left, which is what a put-away that
/// failed part way leaves (R24). Without `survey` a put-away one can't be
/// judged, and is not listed.
async fn is_leftover(
    core: &Core,
    workspace: &Workspace,
    survey: Option<&Survey>,
) -> Result<bool, ApiError> {
    if matches!(
        workspace.state,
        WorkspaceState::Creating | WorkspaceState::Removing
    ) {
        return Ok(false);
    }
    let Some(made) = core.store.workspace_made_at(workspace.id.clone()).await? else {
        return Ok(false);
    };
    if core.sweep_now() - made < LEFTOVER_AGE {
        return Ok(false);
    }
    if workspace.state != WorkspaceState::PutAway {
        return Ok(true);
    }
    match survey {
        Some(survey) => Ok(workspace::worktree_gone_in(&survey.worktrees, &workspace.path).await?),
        None => Ok(false),
    }
}

/// The `branch` items: each local `ersa/…` branch that no workspace names
/// and no worktree has checked out, once for each repository.
async fn branch_items(
    core: &Core,
    world: &World,
    surveys: &BTreeMap<String, Survey>,
) -> Result<Vec<CleanupItem>, ApiError> {
    let named = named_branches(core, world).await?;
    let mut seen: HashSet<&str> = HashSet::new();
    let mut items = Vec::new();
    // By project id: two projects in one repository list its branches
    // under the first.
    for (project_id, survey) in surveys {
        if !seen.insert(&survey.common) {
            continue;
        }
        let checked_out = checked_out(survey);
        for branch in &survey.ersa_branches {
            if !named.contains(branch) && !checked_out.contains(branch) {
                items.push(branch_item(project_id, branch));
            }
        }
    }
    Ok(items)
}

/// Every branch a workspace names: its own, and the one a put-away
/// workspace had checked out, which its put-back needs.
async fn named_branches(core: &Core, world: &World) -> Result<HashSet<String>, ApiError> {
    let mut named: HashSet<String> = world.workspaces.iter().map(|w| w.vcs_ref.clone()).collect();
    for workspace in &world.workspaces {
        if workspace.state != WorkspaceState::PutAway {
            continue;
        }
        if let Some(Checkout::Branch(branch)) =
            core.store.put_away_checkout(workspace.id.clone()).await?
        {
            named.insert(branch);
        }
    }
    Ok(named)
}

/// The branches the repository's worktrees have checked out.
fn checked_out(survey: &Survey) -> HashSet<String> {
    survey
        .worktrees
        .iter()
        .filter_map(|w| w.branch.as_deref()?.strip_prefix("refs/heads/"))
        .map(str::to_string)
        .collect()
}

/// The `folder` items: each real folder in a project's folder under the
/// worktrees root that no workspace and no git worktree record names, and
/// the projects whose folder could not be read.
async fn folder_items(
    core: &Core,
    world: &World,
    surveys: &BTreeMap<String, Survey>,
) -> Result<(Vec<CleanupItem>, Vec<CleanupUnread>), ApiError> {
    let mut items = Vec::new();
    let mut unread = Vec::new();
    // What can't be read leaves every project's folder unread, as a
    // project's git calls that fail leave its own.
    let read_all = |found: ApiError| -> Vec<CleanupUnread> {
        world
            .git_projects()
            .iter()
            .map(|p| CleanupUnread {
                project_id: p.id.clone(),
                message: found.0.message.clone(),
            })
            .collect()
    };
    let root = match worktrees_root(core).await {
        Ok(Some(root)) => root,
        Ok(None) => return Ok((items, unread)),
        Err(e) => return Ok((items, read_all(e))),
    };
    let named = match world.workspace_keys().await {
        Ok(named) => named,
        Err(e) => return Ok((items, read_all(e))),
    };
    let mut done: HashSet<String> = HashSet::new();
    for project in world.git_projects() {
        let folder = root.join(&project.name);
        if !done.insert(platform::path_key(&folder.display().to_string())) {
            continue;
        }
        // Every git project sharing the folder must have been read: a
        // record of any of them may name a folder there.
        let sharing = world.sharing_folder(&root, &project.name);
        let read: Option<Vec<Survey>> = sharing
            .iter()
            .filter(|p| p.vcs == Vcs::Git)
            .map(|p| surveys.get(&p.id).cloned())
            .collect();
        let Some(read) = read else {
            continue;
        };
        let names = match workspace::real_folders_in(&folder).await {
            Ok(names) => names,
            Err(e) => {
                unread.push(CleanupUnread {
                    project_id: project.id.clone(),
                    message: ApiError::from(e).0.message,
                });
                continue;
            }
        };
        for name in names {
            let path = folder.join(&name).display().to_string();
            match unidentified(&path, &named, &read).await {
                Ok(true) => items.push(folder_item(&project.id, &name, path)),
                Ok(false) => {}
                Err(e) => {
                    unread.push(CleanupUnread {
                        project_id: project.id.clone(),
                        message: e.0.message,
                    });
                    break;
                }
            }
        }
    }
    Ok((items, unread))
}

/// Whether no workspace (by `named`, their keys) and no worktree record in
/// `surveys` names the folder at `path`.
async fn unidentified(
    path: &str,
    named: &HashSet<String>,
    surveys: &[Survey],
) -> Result<bool, ApiError> {
    let keys = workspace::location_keys(vec![path.to_string()]).await?;
    if keys.iter().any(|key| key.is_empty() || named.contains(key)) {
        return Ok(false);
    }
    for survey in surveys {
        if workspace::names_folder(&survey.worktrees, path).await {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The worktrees root, as `workspace.create` makes workspaces under it;
/// `None` when it does not exist, so nothing is in it.
async fn worktrees_root(core: &Core) -> Result<Option<PathBuf>, ApiError> {
    let root = settings::workspaces_root(&core.store.settings().await?, &core.config.data_dir);
    Ok(workspace::canonical_folder(root).await?)
}

/// What the list reads of `project`'s repository, its git calls given 10
/// seconds together.
async fn survey(core: &Core, project: &Project) -> Result<Survey, ApiError> {
    let transport = transport(core, project)?;
    Ok(workspace::git_cleanup_survey(transport.as_ref(), &project.path).await?)
}

fn transport(
    core: &Core,
    project: &Project,
) -> Result<std::sync::Arc<dyn ersa_adapter::Transport>, ApiError> {
    core.transport_for(&project.host_id)
        .ok_or_else(|| handlers::unreachable_host(&project.host_id))
}

// -- Items ----------------------------------------------------------------

fn workspace_id(workspace: &str) -> String {
    format!("{WORKSPACE_ID}{workspace}")
}

fn item(id: String, kind: CleanupKind, project_id: &str) -> CleanupItem {
    CleanupItem {
        id,
        kind,
        project_id: project_id.to_string(),
        workspace_id: None,
        path: None,
        branch: None,
        session_ids: None,
        last_activity_at: None,
    }
}

/// A branch name holds no `:`, so the id splits back at its first one.
fn branch_item(project_id: &str, branch: &str) -> CleanupItem {
    CleanupItem {
        branch: Some(branch.to_string()),
        ..item(
            format!("{BRANCH_ID}{project_id}:{branch}"),
            CleanupKind::Branch,
            project_id,
        )
    }
}

fn folder_item(project_id: &str, name: &str, path: String) -> CleanupItem {
    CleanupItem {
        path: Some(path),
        ..item(
            format!("{FOLDER_ID}{project_id}:{name}"),
            CleanupKind::Folder,
            project_id,
        )
    }
}
