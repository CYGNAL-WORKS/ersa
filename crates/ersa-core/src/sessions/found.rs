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
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ersa_adapter::{Adapter, BackendChoices, Transport};
use ersa_protocol::core_api::{Workspace, WorkspaceState};

use super::{Handle, STARTUP_SUMMARIES, SUMMARY_READ, SessionManager, UNKNOWN_TIME, lock};
use super::{SessionError, read_summary};
use crate::admission::{Command, Report, UnderWay};
use crate::persistence::{self, SessionRow};
use crate::platform;

/// How long asking a backend for the records of one folder may take. A
/// folder on a file system that stopped answering is skipped for this list,
/// its sessions kept as they were.
const FOLDER_LISTING: Duration = Duration::from_secs(5);

/// The folders whose listing is still under way, by backend and path. A
/// listing past its time keeps its thread until the file system answers, so
/// a folder still being listed is not listed again: a file system that
/// stopped answering holds one thread per folder, however often Core lists.
pub(super) type InFlight = Arc<Mutex<HashSet<(String, String)>>>;

/// The exclusive operations of a workspace's found sessions, held while a
/// removal takes their rows ([`SessionManager::quiet_found`]).
#[must_use]
pub struct QuietFound {
    /// Each session's row as it was when its operation was taken, and
    /// the operation, which ends when dropped.
    held: Vec<(SessionRow, UnderWay)>,
}

/// One folder to list: a `ready` workspace, for one backend.
struct Folder {
    adapter: Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
    workspace: Workspace,
    host_id: String,
}

impl SessionManager {
    /// Brings the sessions started in the backend's own tools up to date with
    /// the backend's records, for `session.list`: every record a registered
    /// backend holds for a `ready` workspace's folder, counting only records
    /// that name that folder as their own, joins the list in that workspace
    /// (FLOW-RESUME-SESSION-R12) under a new Ersa id that is then kept
    /// (R05), and a session found before whose record is gone leaves it
    /// (R06). A session Ersa started is never added or left out here.
    ///
    /// Every folder is listed at once, and the backend asked about missing
    /// sessions at once, each within a bound and outside any lock. Only
    /// recording and forgetting take the listing turn, so two lists never
    /// record one session twice; a delete takes the same turn and counts
    /// itself, and a listing a delete overlapped records nothing new, so a
    /// deleted session is never found again. Whether a workspace takes a
    /// found session is admission's to decide, so a removal needs no turn.
    pub async fn find_elsewhere(
        &self,
        transport_for: impl Fn(&str) -> Option<Arc<dyn Transport>>,
    ) -> Result<(), SessionError> {
        // A delete that lands while the folders are read may have removed a
        // record this listing still holds: what it lists is then not
        // recorded as new this time, and the next list reads again.
        let deletes_before = self.deletes.load(Ordering::SeqCst);
        let mut folders = Vec::new();
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
                    folders.push(Folder {
                        adapter: Arc::clone(adapter),
                        transport: Arc::clone(&transport),
                        workspace: workspace.clone(),
                        host_id: project.host_id.clone(),
                    });
                }
            }
        }

        let mut listings = tokio::task::JoinSet::new();
        for (index, folder) in folders.iter().enumerate() {
            let listing = list_folder(
                Arc::clone(&self.listing_in_flight),
                Arc::clone(&folder.adapter),
                Arc::clone(&folder.transport),
                folder.workspace.clone(),
            );
            listings.spawn(async move { (index, listing.await) });
        }
        let mut listed: Vec<Option<HashSet<String>>> = vec![None; folders.len()];
        while let Some(done) = listings.join_next().await {
            if let Ok((index, records)) = done {
                listed[index] = records;
            }
        }

        let mut summaries = tokio::task::JoinSet::new();
        let mut missing = Vec::new();
        {
            let _listing = self.listing.lock().await;
            let deleted_meanwhile = self.deletes.load(Ordering::SeqCst) != deletes_before;
            // Every session Core has stored, dormant and deleting ones
            // included: a record that is one of them is never found as another.
            let mut known: HashSet<(String, String)> = self
                .store
                .sessions()
                .await?
                .into_iter()
                .filter_map(|row| Some((row.backend, row.backend_session_id?)))
                .collect();
            for (folder, listed) in folders.iter().zip(&listed) {
                // A folder that could not be listed says nothing.
                let Some(listed) = listed else {
                    continue;
                };
                for backend_id in listed.iter().filter(|_| !deleted_meanwhile) {
                    let key = (folder.adapter.backend().to_string(), backend_id.clone());
                    if !known.insert(key) {
                        continue;
                    }
                    if let Some(handle) = self.record_found(folder, backend_id).await? {
                        // Gone again already: nothing to read for it.
                        let Some(row) = handle.stored_row() else {
                            continue;
                        };
                        let (store, tiles) = (self.store.clone(), self.tiles.clone());
                        let (adapter, transport) =
                            (Arc::clone(&folder.adapter), Arc::clone(&folder.transport));
                        summaries.spawn(async move {
                            read_summary(&store, &handle, &tiles, adapter, transport, row).await;
                        });
                    }
                }
                missing.extend(
                    self.found_missing(folder, listed)
                        .into_iter()
                        .map(|handle| (handle, folder)),
                );
            }
        }
        self.leave_out_gone(missing).await?;

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

    /// Records the session `backend_id` found in `folder`, and loads it.
    /// `None` when it was not recorded: admission did not take it into the
    /// workspace, which stopped being `ready`, or the session was recorded
    /// meanwhile.
    async fn record_found(
        &self,
        folder: &Folder,
        backend_id: &str,
    ) -> Result<Option<Arc<Handle>>, SessionError> {
        let row = SessionRow {
            id: persistence::new_id("s"),
            workspace_id: folder.workspace.id.clone(),
            host_id: folder.host_id.clone(),
            backend: folder.adapter.backend().to_string(),
            backend_session_id: Some(backend_id.to_string()),
            mode: None,
            model: None,
            effort: None,
            permission_mode: None,
            model_at: None,
            effort_at: None,
            permission_mode_at: None,
            cwd: folder.workspace.path.clone(),
            archived: false,
            deleting: None,
            pinned: false,
            section_id: None,
            last_activity_at: None,
            forked_from: None,
            forked_through: None,
            started_elsewhere: true,
        };
        let command = Command::CreateSession {
            workspace: row.workspace_id.clone(),
            session: row.id.clone(),
            found: true,
        };
        let Ok(admitted) = self.admission.command(command).await else {
            return Ok(None);
        };
        if !self.store.insert_found_session(row.clone()).await? {
            admitted.end(Report::SessionGone { session: row.id });
            return Ok(None);
        }
        admitted.end(Report::SessionStored {
            row: Box::new(row.clone()),
        });
        if let Some(gate) = &self.found_gate {
            gate.stop().await;
        }
        // As at startup: with no request of Core's, the permission mode is
        // the backend's default until the record says otherwise.
        let default = BackendChoices::default_of(&folder.adapter.choices().permission_modes);
        let default = (!default.is_empty()).then_some(default);
        let admission = Arc::clone(&self.admission);
        let started = UNKNOWN_TIME.to_string();
        let handle = Arc::new(Handle::new(&row, started, admission, default));
        // Loaded only while admission still has it. A removal that took its
        // row since tells admission before it reads this registry
        // ([`SessionManager::forget_found`]), so the session is either
        // loaded before the removal looks, and goes with it, or is not
        // loaded: it is never left listed in what was removed
        // (FLOW-RESUME-SESSION-R13).
        let mut sessions = lock(&self.sessions);
        if !self.admission.has_session(&handle.id) {
            return Ok(None);
        }
        sessions.insert(handle.id.clone(), Arc::clone(&handle));
        drop(sessions);
        Ok(Some(handle))
    }

    /// The sessions found before in `folder` that its listing no longer
    /// holds. Only those started elsewhere: a session Ersa started is never
    /// left out, since a new session or fork has no record until its first
    /// message.
    fn found_missing(&self, folder: &Folder, listed: &HashSet<String>) -> Vec<Arc<Handle>> {
        lock(&self.sessions)
            .values()
            .filter(|handle| {
                handle.stored_row().is_some_and(|row| {
                    row.started_elsewhere
                        && row.workspace_id == folder.workspace.id
                        && row.backend == folder.adapter.backend()
                        && row.deleting.is_none()
                        && row
                            .backend_session_id
                            .as_ref()
                            .is_some_and(|id| !listed.contains(id))
                })
            })
            .cloned()
            .collect()
    }

    /// Leaves out each of `missing` whose backend, asked, says it has no
    /// record (FLOW-RESUME-SESSION-R06). Asked all at once, each within a
    /// bound; one the backend cannot say for stays, and so does one Core is
    /// running or deleting by the time it is forgotten.
    async fn leave_out_gone(
        &self,
        missing: Vec<(Arc<Handle>, &Folder)>,
    ) -> Result<(), SessionError> {
        let mut asked = tokio::task::JoinSet::new();
        for (handle, folder) in missing {
            let Some(backend_id) = handle.stored_row().and_then(|row| row.backend_session_id)
            else {
                continue;
            };
            let (adapter, transport) = (Arc::clone(&folder.adapter), Arc::clone(&folder.transport));
            let cwd = folder.workspace.path.clone();
            asked.spawn(async move {
                let read = adapter.summarize_record(transport, &cwd, &backend_id);
                let gone = matches!(tokio::time::timeout(SUMMARY_READ, read).await, Ok(Ok(None)));
                (handle, gone)
            });
        }
        while let Some(done) = asked.join_next().await {
            let Ok((handle, true)) = done else {
                continue;
            };
            let _listing = self.listing.lock().await;
            // Gone already, by a removal or a delete: nothing to leave out.
            let Ok(_exclusive) = self.exclusive(&handle.id).await else {
                continue;
            };
            // Gone since, though the exclusive operation was given: none.
            let Some(row) = handle.stored_row() else {
                continue;
            };
            if handle.live().is_some() || row.deleting.is_some() {
                continue;
            }
            self.forget_row(&handle.id).await?;
            self.let_go(&handle, &row).await;
        }
        Ok(())
    }

    /// Takes the exclusive operation of each found session in workspace
    /// `workspace_id`, waiting for any under way, before a removal deletes
    /// their rows: an archive or an arrangement still writing one finishes
    /// first, and nothing else begins on them. Asked of admission, which
    /// has every one whose row is stored, loaded here or not, and kept with
    /// each row as it then is: admission keeps none once the workspace is
    /// gone. Given to
    /// [`SessionManager::forget_found`] once admission was told the
    /// workspace is gone; dropped when the delete fails, which lets them go
    /// on.
    pub async fn quiet_found(&self, workspace_id: &str) -> QuietFound {
        let mut quiet = Vec::new();
        for found in self.admission.found_in(workspace_id) {
            // One that is gone already has nothing under way.
            let Ok(exclusive) = self.exclusive(&found.id).await else {
                continue;
            };
            // Read again with the operation held: nothing writes it now.
            if let Some(row) = self.admission.row(&found.id) {
                quiet.push((row, exclusive));
            }
        }
        QuietFound { held: quiet }
    }

    /// Lets go of the found sessions `quiet` names that are loaded, whose
    /// rows went with their workspace, their records left
    /// (FLOW-RESUME-SESSION-R13). Called once admission was told the
    /// workspace is gone: a list that stored a found session there loads it
    /// before this reads the registry, or finds admission without it and
    /// loads nothing. None is running, or admission had refused the removal, and
    /// none has an operation under way: `quiet` holds each one's, taken
    /// before the rows went, and lets go last. A request that waited there
    /// finds the session gone.
    pub async fn forget_found(&self, quiet: QuietFound) {
        for (row, _exclusive) in &quiet.held {
            // One never loaded (its backend has no adapter here, or a list
            // found admission without it) has nothing to let go of.
            let loaded = lock(&self.sessions).get(&row.id).cloned();
            if let Some(handle) = loaded {
                self.let_go(&handle, row).await;
            }
        }
        drop(quiet);
    }
}

/// The sessions `adapter`'s backend holds records of for `workspace`'s
/// folder, counting only records that name that folder as their own: a
/// backend may file two folders' records together. `None` when the folder
/// could not be listed, or is still being listed from an earlier call, so
/// nothing is concluded from it.
async fn list_folder(
    in_flight: InFlight,
    adapter: Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
    workspace: Workspace,
) -> Option<HashSet<String>> {
    let key = (adapter.backend().to_string(), workspace.path.clone());
    if !lock(&in_flight).insert(key.clone()) {
        return None;
    }
    // Its own task, which lets the folder go only when the listing returns,
    // past the bound too.
    let path = workspace.path.clone();
    let listing = {
        let adapter = Arc::clone(&adapter);
        tokio::spawn(async move {
            let records = adapter.list_records(transport, &path).await;
            lock(&in_flight).remove(&key);
            records
        })
    };
    let records = match tokio::time::timeout(FOLDER_LISTING, listing).await {
        Ok(Ok(Ok(records))) => records,
        Ok(Ok(Err(e))) => {
            tracing::warn!(workspace = %workspace.id, backend = adapter.backend(), error = %e, "could not list the backend's sessions for the folder");
            return None;
        }
        Ok(Err(e)) => {
            tracing::warn!(workspace = %workspace.id, backend = adapter.backend(), error = %e, "listing the backend's sessions for the folder failed");
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
            .filter(|record| platform::path_key(&record.cwd) == folder)
            .map(|record| record.backend_session_id)
            .collect(),
    )
}
