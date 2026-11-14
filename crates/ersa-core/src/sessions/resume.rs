//! Starting a stopped session's backend again (FLOW-RESUME-SESSION): by
//! `session.resume`, and by a `session.send` to a session that is not
//! running.
//!
//! The session continues in the same record. Its view goes on from the
//! events it already holds: the record's, then the live stream of the new
//! run. Claude Code repeats none of the record on its stream when it resumes
//! (fixture 2.1.316/29-resume-while-open), and a live line whose message the
//! record already yielded adds nothing (PROTO-AGENT-EVENTS-R12).

use std::path::PathBuf;

use ersa_adapter::ForkOf;
use serde_json::Value;

use super::*;

/// How long Core waits to learn whether the session's folder is there.
const FOLDER_CHECK: Duration = Duration::from_secs(10);

/// What starting a stopped session again takes that the session manager
/// does not keep: the host's transport and what is known of the backend and
/// the project, looked up as for a new session.
pub struct Resumption {
    pub transport: Arc<dyn Transport>,
    /// The version detection found, until the backend announces its own.
    pub backend_version: String,
    /// Whether the user trusts the project now (ADR 0018).
    pub project_trusted: bool,
    /// The project's root, as a path on the session's host.
    pub project_path: String,
}

/// Whether the caller has just looked at the session's folder itself, so a
/// resume need not look again.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FolderChecked {
    Yes,
    No,
}

/// How the backend is started again, from what its record says.
struct HowToStart {
    /// The id a new session, or a fork made again, starts under.
    backend_session_id: Option<String>,
    /// The id of the record the backend continues.
    resume: Option<String>,
    fork_of: Option<ForkOf>,
}

impl SessionManager {
    /// `session.resume`: starts the session's backend again, in its own
    /// folder, so it continues in the same record (FLOW-RESUME-SESSION-R01).
    /// A session already running is left as it is.
    pub async fn resume(&self, id: &str, how: Resumption) -> Result<Session, SessionError> {
        let handle = self.handle(id)?;
        self.start_again(&handle, how, FolderChecked::No).await?;
        Ok(handle.describe())
    }

    /// Whether the session's backend is running, once a run that has ended
    /// has wound down.
    pub async fn is_running(&self, id: &str) -> Result<bool, SessionError> {
        let handle = self.handle(id)?;
        Ok(current_run(&handle).await?.is_some())
    }

    /// Starts the backend of `handle` again unless it is running. Returns
    /// once the session is live; the start itself shows up as events.
    pub(super) async fn start_again(
        &self,
        handle: &Arc<Handle>,
        how: Resumption,
        folder: FolderChecked,
    ) -> Result<(), SessionError> {
        // Waited for before taking `op`, so nothing else on the session
        // queues behind the wait.
        current_run(handle).await?;
        // Held throughout, so a stop, a removal or a view opened meanwhile
        // waits for the session to be live, and two resumes start it once.
        let _op = handle.op.lock().await;
        if handle.removed.load(Ordering::SeqCst) {
            return Err(SessionError::NotFound(handle.id.clone()));
        }
        if handle.live().is_some() {
            return Ok(());
        }
        self.refuse_if_deleting(handle)?;
        let row = lock(&handle.row).clone();
        self.refuse_if_claimed(&row.workspace_id)?;
        if self.closing.load(Ordering::SeqCst) {
            return Err(SessionError::InvalidState(
                "Core is shutting down; no session can start".into(),
            ));
        }
        if folder == FolderChecked::No {
            check_folder(handle).await?;
        }
        let adapter = self.adapter_of(&row.backend)?;
        let start = self
            .how_to_start(handle, &adapter, &how.transport, &row)
            .await?;

        // Accepted. An archived session is unarchived before its backend
        // starts (PROTO-CORE-API-R43), stored first, as `set_archived` does.
        if row.archived {
            self.store
                .set_session_archived(handle.id.clone(), false)
                .await?;
            lock(&handle.row).archived = false;
        }

        // The view clients are reading goes on, so they see the new run
        // after what they have; with nobody reading, the record is read
        // afresh, as opening the session would.
        let reading = {
            let hub = lock(&handle.hub);
            hub.view.is_some() && !hub.subscribers.is_empty()
        };
        if !reading {
            let transport = Arc::clone(&how.transport);
            let transport_for = move |_: &str| Some(Arc::clone(&transport));
            let (view, events) = self.open_view(handle, &transport_for).await?;
            lock(&handle.hub).install(&handle.id, view, &events);
        }
        if let Some(view) = lock(&handle.hub).view.as_mut() {
            view.resume(adapter.projector());
        }
        // The raw log holds the lines since Core last started the backend.
        *lock(&handle.raw) = RawLog::default();

        let permission_mode = row
            .permission_mode
            .clone()
            .unwrap_or_else(|| BackendChoices::default_of(&adapter.choices().permission_modes));
        let config = SessionConfig {
            cwd: row.cwd.clone(),
            permission_mode,
            // What the session was started with or last changed to, as
            // Core stored it (PROTO-CORE-API-R16).
            model: row.model.clone(),
            effort: row.effort.clone(),
            project_trusted: how.project_trusted,
            project_path: how.project_path,
            backend_session_id: start.backend_session_id,
            fork_of: start.fork_of,
            resume: start.resume,
            extra: Value::Null,
        };
        // Launched under the lock `stop_all` takes, as a new session is.
        let sessions = lock(&self.sessions);
        if self.closing.load(Ordering::SeqCst) {
            return Err(SessionError::InvalidState(
                "Core is shutting down; no session can start".into(),
            ));
        }
        self.launch(
            Arc::clone(handle),
            adapter,
            how.transport,
            config,
            how.backend_version,
            Launched::Resumed,
        );
        drop(sessions);
        Ok(())
    }

    /// Whether the backend continues the session's record or, with no record
    /// yet, starts it again as it first started (PROTO-CLAUDE-CODE-STREAM,
    /// Invocation): a new session under the id Core stored, or a fork
    /// copying its original again.
    async fn how_to_start(
        &self,
        handle: &Handle,
        adapter: &Arc<dyn Adapter>,
        transport: &Arc<dyn Transport>,
        row: &SessionRow,
    ) -> Result<HowToStart, SessionError> {
        let Some(own) = row.backend_session_id.clone() else {
            // The backend never named the session, so it has no record: it
            // starts as new, under an id Core chooses and stores first when
            // the backend takes one.
            let chosen = adapter.new_session_id();
            if let Some(id) = &chosen {
                self.store
                    .set_backend_session_id(handle.id.clone(), id.clone())
                    .await?;
                lock(&handle.row).backend_session_id = Some(id.clone());
            }
            return Ok(HowToStart {
                backend_session_id: chosen,
                resume: None,
                fork_of: None,
            });
        };
        let version = adapter.record_version(Arc::clone(transport), &row.cwd, &own);
        let no_record = matches!(
            tokio::time::timeout(RECORD_READ, version).await,
            Ok(Ok(None))
        );
        if !no_record {
            // A record that can't be looked at is resumed all the same: the
            // backend then says what is wrong with it.
            return Ok(HowToStart {
                backend_session_id: None,
                resume: Some(own),
                fork_of: None,
            });
        }
        let fork_of = row.forked_from.clone().map(|original| ForkOf {
            original,
            name: lock(&handle.presented).title.clone(),
        });
        Ok(HowToStart {
            backend_session_id: Some(own),
            resume: None,
            fork_of,
        })
    }
}

/// What looking at a session's folder found: whether it is a folder, or
/// why it could not be looked at.
pub(super) type FolderLook = Result<bool, String>;

/// Refuses with `folder_gone` when the session's folder is not an existing
/// folder (FLOW-RESUME-SESSION-R03). Core looks for itself, before the
/// backend starts. Only the local host has sessions today. One look per
/// session at a time: a look stuck on a drive that stopped answering is
/// joined, never started again beside it ([`read_once`]).
pub(super) async fn check_folder(handle: &Handle) -> Result<(), SessionError> {
    let cwd = lock(&handle.row).cwd.clone();
    let path = PathBuf::from(&cwd);
    let start = move || {
        tokio::task::spawn_blocking(move || match std::fs::metadata(&path) {
            Ok(meta) => Ok(meta.is_dir()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.to_string()),
        })
    };
    // A look that finished a moment ago is as good as a new one.
    match read_once(&handle.folder_check, FOLDER_CHECK, start, async |_| true).await {
        Some(Ok(true)) => Ok(()),
        Some(Ok(false)) => Err(SessionError::FolderGone(format!(
            "the session's folder {cwd} no longer exists, so the session can't be resumed"
        ))),
        // There, perhaps, but not readable: say why rather than call it gone.
        Some(Err(e)) => Err(SessionError::InvalidState(format!(
            "the session's folder {cwd} can't be read: {e}"
        ))),
        None => Err(SessionError::InvalidState(format!(
            "the session's folder {cwd} did not answer within {FOLDER_CHECK:?}"
        ))),
    }
}

/// The session's run, unless it has ended and is only winding down, which
/// is waited for first, as `session.remove` waits: a client may have seen
/// `session.ended` before Core let go of the run, and a resume or a send
/// then starts the session again rather than finding it still running.
pub(super) async fn current_run(handle: &Handle) -> Result<Option<Arc<Live>>, SessionError> {
    match handle.live() {
        Some(live) if live.sink.has_ended() => {
            if wait_finished(&live, STOP_TIMEOUT).await.is_err() {
                return Err(SessionError::Internal(format!(
                    "session {} has ended, but Core has not finished with it within {STOP_TIMEOUT:?}",
                    handle.id
                )));
            }
            Ok(handle.live())
        }
        run => Ok(run),
    }
}
