//! Sessions started in the backend's own tools (FLOW-RESUME-SESSION, step 5):
//! found in the backend's records for a project's folder and its workspaces'
//! folders each time Core lists sessions, kept under one Ersa id with the
//! user's arrangement of them, left out once their record is gone, and
//! forgotten with their workspace.
//!
//! Core stores only what finds such a session again and the arrangement (ADR
//! 0033): its title, times and settings are read from its record like any
//! other session's.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ersa_adapter::{Adapter, BackendChoices, Transport};
use ersa_protocol::core_api::{Workspace, WorkspaceState};

use super::{Handle, RECORD_READ, STARTUP_SUMMARIES, SessionManager, UNKNOWN_TIME, lock};
use super::{SessionError, read_summary};
use crate::persistence::{self, SessionRow};
use crate::platform;

/// How long asking a backend for the records of one folder may take. A
/// folder on a file system that stopped answering is skipped for this list,
/// its sessions kept as they were.
const FOLDER_LISTING: Duration = Duration::from_secs(5);

impl SessionManager {
    /// Brings the sessions started in the backend's own tools up to date with
    /// the backend's records, for `session.list`: every record a registered
    /// backend holds for a `ready` workspace's folder, counting only records
    /// that name that folder as their own, joins the list in that workspace
    /// (FLOW-RESUME-SESSION-R12) under a new Ersa id that is then kept
    /// (R05), and a session found before whose record is gone leaves it
    /// (R06). A session Ersa started is never added or left out here.
    ///
    /// One listing at a time, so two never record one session twice; a
    /// delete takes the same turn, so a list cannot find a record again
    /// that the delete removed meanwhile.
    pub async fn find_elsewhere(
        &self,
        transport_for: impl Fn(&str) -> Option<Arc<dyn Transport>>,
    ) -> Result<(), SessionError> {
        let _listing = self.listing.lock().await;
        // Every session Core has stored, dormant and deleting ones included:
        // a record that is one of them is never found as another.
        let mut known: HashSet<(String, String)> = self
            .store
            .sessions()
            .await?
            .into_iter()
            .filter_map(|row| Some((row.backend, row.backend_session_id?)))
            .collect();
        let mut summaries = tokio::task::JoinSet::new();
        for project in self.store.projects().await? {
            let Some(transport) = transport_for(&project.host_id) else {
                continue;
            };
            for workspace in self.store.workspaces(project.id.clone()).await? {
                // A workspace still being made, or being removed, gains no
                // session; one in error is left as it is.
                if workspace.state != WorkspaceState::Ready {
                    continue;
                }
                for adapter in &self.adapters {
                    let Some(listed) =
                        list_folder(adapter, Arc::clone(&transport), &workspace).await
                    else {
                        continue;
                    };
                    for backend_id in &listed {
                        let key = (adapter.backend().to_string(), backend_id.clone());
                        if known.contains(&key) {
                            continue;
                        }
                        known.insert(key);
                        if let Some(handle) = self
                            .record_found(
                                adapter.as_ref(),
                                &workspace,
                                &project.host_id,
                                backend_id,
                            )
                            .await?
                        {
                            let row = lock(&handle.row).clone();
                            let (store, tiles) = (self.store.clone(), self.tiles.clone());
                            let (adapter, transport) =
                                (Arc::clone(adapter), Arc::clone(&transport));
                            summaries.spawn(async move {
                                read_summary(&store, &handle, &tiles, adapter, transport, row)
                                    .await;
                            });
                        }
                    }
                    self.leave_out_gone(adapter, Arc::clone(&transport), &workspace, &listed)
                        .await?;
                }
            }
        }
        // A new session's title and date come from its record, read before
        // the list answers so it shows titled and in its place; never waited
        // on past a bound, as at startup.
        let all = async { while summaries.join_next().await.is_some() {} };
        if tokio::time::timeout(STARTUP_SUMMARIES, all).await.is_err() {
            tracing::warn!(
                "reading the titles of sessions found in the backend's records took too long"
            );
        }
        Ok(())
    }

    /// Records the session `backend_id` found in `workspace`'s folder, and
    /// loads it. `None` when it was not recorded: the workspace stopped
    /// being `ready`, or the session was recorded meanwhile.
    async fn record_found(
        &self,
        adapter: &dyn Adapter,
        workspace: &Workspace,
        host_id: &str,
        backend_id: &str,
    ) -> Result<Option<Arc<Handle>>, SessionError> {
        let mut row = SessionRow {
            id: persistence::new_id("s"),
            workspace_id: workspace.id.clone(),
            host_id: host_id.to_string(),
            backend: adapter.backend().to_string(),
            backend_session_id: Some(backend_id.to_string()),
            mode: None,
            model: None,
            effort: None,
            permission_mode: None,
            model_at: None,
            effort_at: None,
            permission_mode_at: None,
            cwd: workspace.path.clone(),
            archived: false,
            deleting: None,
            pinned: false,
            section_id: None,
            last_activity_at: None,
            forked_from: None,
            forked_through: None,
            started_elsewhere: true,
        };
        if !self.store.insert_found_session(row.clone()).await? {
            return Ok(None);
        }
        // As at startup: with no request of Core's, the permission mode is
        // the backend's default until the record says otherwise.
        let default = BackendChoices::default_of(&adapter.choices().permission_modes);
        row.permission_mode = (!default.is_empty()).then_some(default);
        let handle = Arc::new(Handle::new(row, UNKNOWN_TIME.to_string()));
        lock(&self.sessions).insert(handle.id.clone(), Arc::clone(&handle));
        Ok(Some(handle))
    }

    /// Leaves out the sessions found before in `workspace`'s folder whose
    /// record `adapter`'s backend no longer holds (FLOW-RESUME-SESSION-R06):
    /// those missing from `listed` that the backend, asked for each, says
    /// have no record. One it cannot say for stays, and so does one Core is
    /// running or deleting.
    async fn leave_out_gone(
        &self,
        adapter: &Arc<dyn Adapter>,
        transport: Arc<dyn Transport>,
        workspace: &Workspace,
        listed: &HashSet<String>,
    ) -> Result<(), SessionError> {
        let missing: Vec<Arc<Handle>> = lock(&self.sessions)
            .values()
            .filter(|handle| {
                let row = lock(&handle.row);
                row.started_elsewhere
                    && row.workspace_id == workspace.id
                    && row.backend == adapter.backend()
                    && row.deleting.is_none()
                    && row
                        .backend_session_id
                        .as_ref()
                        .is_some_and(|id| !listed.contains(id))
            })
            .cloned()
            .collect();
        for handle in missing {
            let _op = handle.op.lock().await;
            if handle.live().is_some() || handle.removed.load(Ordering::SeqCst) {
                continue;
            }
            let Some(backend_id) = lock(&handle.row).backend_session_id.clone() else {
                continue;
            };
            let asked =
                adapter.summarize_record(Arc::clone(&transport), &workspace.path, &backend_id);
            if let Ok(Ok(None)) = tokio::time::timeout(RECORD_READ, asked).await {
                self.store.delete_session(handle.id.clone()).await?;
                self.let_go(&handle).await;
            }
        }
        Ok(())
    }

    /// Lets go of the sessions found in workspace `workspace_id`'s folder
    /// whose rows went with the workspace (FLOW-RESUME-SESSION-R13).
    pub async fn forget_found(&self, workspace_id: &str) -> Result<(), SessionError> {
        let found: Vec<Arc<Handle>> = lock(&self.sessions)
            .values()
            .filter(|handle| {
                let row = lock(&handle.row);
                row.started_elsewhere && row.workspace_id == workspace_id
            })
            .cloned()
            .collect();
        for handle in found {
            if self.store.session(handle.id.clone()).await?.is_none() {
                self.let_go(&handle).await;
            }
        }
        Ok(())
    }

    /// A session found in the backend's records that Core is running in
    /// workspace `workspace_id`, which the workspace cannot go without:
    /// no backend is left running in a removed folder.
    pub fn running_found(&self, workspace_id: &str) -> Option<String> {
        lock(&self.sessions)
            .values()
            .find(|handle| {
                let row = lock(&handle.row);
                row.started_elsewhere && row.workspace_id == workspace_id && handle.live().is_some()
            })
            .map(|handle| handle.id.clone())
    }
}

/// The sessions `adapter`'s backend holds records of for `workspace`'s
/// folder, counting only records that name that folder as their own: a
/// backend may file two folders' records together. `None` when the folder
/// could not be listed, so nothing is concluded from it.
async fn list_folder(
    adapter: &Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
    workspace: &Workspace,
) -> Option<HashSet<String>> {
    let listing = adapter.list_records(transport, &workspace.path);
    let records = match tokio::time::timeout(FOLDER_LISTING, listing).await {
        Ok(Ok(records)) => records,
        Ok(Err(e)) => {
            tracing::warn!(workspace = %workspace.id, backend = adapter.backend(), error = %e, "could not list the backend's sessions for the folder");
            return None;
        }
        Err(_) => {
            tracing::warn!(workspace = %workspace.id, backend = adapter.backend(), "listing the backend's sessions for the folder took too long");
            return None;
        }
    };
    let folder = platform::path_key(&workspace.path);
    Some(
        records
            .into_iter()
            .filter(|record| {
                record
                    .cwd
                    .as_deref()
                    .is_some_and(|cwd| platform::path_key(cwd) == folder)
            })
            .map(|record| record.backend_session_id)
            .collect(),
    )
}
