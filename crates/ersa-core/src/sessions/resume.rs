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
use ersa_protocol::core_api::{SendPhase, SendProgressParams};
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
    /// Where the session's workspace comes back when it is put away.
    pub put_back: PutBack,
}

/// Whether the caller has just put the session's workspace back and looked
/// at its folder itself, so a resume need do neither again.
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
    /// For a fork made again: the operation that marks its original as
    /// being forked until the fork's backend is ready, as `session.fork`
    /// has (FLOW-RESUME-SESSION-R10).
    forking: Option<Arc<UnderWay>>,
}

impl SessionManager {
    /// `session.resume`: starts the session's backend again, in its own
    /// folder, so it continues in the same record (FLOW-RESUME-SESSION-R01).
    /// A session already running is left as it is.
    pub async fn resume(&self, id: &str, how: Resumption) -> Result<Session, SessionError> {
        let handle = self.handle(id)?;
        self.start_again(&handle, how, FolderChecked::No).await?;
        handle.session()
    }

    /// Whether the session's backend is running, once a run that has ended
    /// has wound down.
    pub async fn is_running(&self, id: &str) -> Result<bool, SessionError> {
        let handle = self.handle(id)?;
        self.runs_once_settled(&handle).await
    }

    /// Whether Core runs the session, once a run that has ended and is only
    /// winding down has been let go of. That is waited for first, held in
    /// admission, up to the stop limit: a client may have seen
    /// `session.ended` before Core let go of the run, and a resume or a
    /// send then starts the session again rather than finding it still
    /// running.
    pub(super) async fn runs_once_settled(&self, handle: &Handle) -> Result<bool, SessionError> {
        match self.admission.run_settled(&handle.id, STOP_TIMEOUT).await {
            Ok(()) => Ok(false),
            Err(Refusal::Running) => Ok(true),
            Err(refusal) => Err(refused_session(refusal, &handle.workspace_id, &handle.id)),
        }
    }

    /// Starts the backend of `handle` again unless it is running. Returns
    /// once the session is live; the start itself shows up as events.
    pub(super) async fn start_again(
        &self,
        handle: &Arc<Handle>,
        how: Resumption,
        folder: FolderChecked,
    ) -> Result<(), SessionError> {
        // The start is the session's exclusive operation throughout, so a
        // stop, a removal or a view opened meanwhile waits for the session
        // to be live, and two resumes start it once. Admission holds it
        // behind whatever is under way on the session, and behind a run
        // that has ended and is winding down (a client may have seen
        // `session.ended` before Core let go of the run), that wait up to
        // the stop limit and holding nothing itself. A session that runs
        // needs no start; nothing starts once Core is shutting down; one
        // that is gone answers `not_found`.
        let workspace_id = handle.workspace_id.clone();
        let begun = self.admission.begin_start(&handle.id, STOP_TIMEOUT).await;
        let _exclusive = match begun {
            Ok(exclusive) => exclusive,
            Err(Refusal::Running) => return Ok(()),
            Err(refusal) => return Err(refused_session(refusal, &workspace_id, &handle.id)),
        };
        self.refuse_if_deleting(handle)?;
        let row = handle.row()?;
        // A put-away workspace is put back first: one being put away, or
        // back for another session, is waited for, not refused as busy
        // (PROTO-CORE-API-R132). A send has put it back already, and said
        // so itself. A resume says so once, as it begins to put back or to
        // wait (PROTO-CORE-API-R146); `report` withdraws that line, if no
        // connection has taken it yet, however this returns, so it never
        // arrives after the resume's answer.
        let report = ResumeReport::new(&self.tiles, &handle.id);
        if folder == FolderChecked::No {
            let there_is = report.putting_back();
            self.put_back(handle, &how, there_is, None).await?;
        }
        self.refuse_if_claimed(&row.workspace_id)?;
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
            self.wrote(handle, RowChange::Archived(false))?;
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
        // The raw log keeps the backend's earlier runs, and this run's lines
        // go on from their last index (PROTO-CORE-API-R136, R137).

        // Weighed against what the record's turns ran with since Core's
        // requests (PROTO-CORE-API-R71).
        let values = self
            .relaunch_values(&adapter, &how.transport, &row, start.resume.as_deref())
            .await;
        let permission_mode = values
            .permission_mode
            .unwrap_or_else(|| BackendChoices::default_of(&adapter.choices().permission_modes));
        let config = SessionConfig {
            cwd: row.cwd.clone(),
            permission_mode,
            model: values.model,
            effort: values.effort,
            project_trusted: how.project_trusted,
            project_path: how.project_path,
            backend_session_id: start.backend_session_id,
            fork_of: start.fork_of,
            resume: start.resume,
            extra: Value::Null,
        };
        // Admission refuses the launch once Core is closing, as a new
        // session's.
        let launched = self.launch(
            Arc::clone(handle),
            adapter,
            how.transport,
            config,
            how.backend_version,
            Launched::Resumed {
                forking: start.forking,
            },
        );
        launched.map_err(|refusal| refused_session(refusal, &row.workspace_id, &handle.id))
    }

    /// The model, effort and permission mode a relaunch passes the backend
    /// (PROTO-CORE-API-R71). Only a session whose record the backend
    /// continues (`resumed`, its backend id) has turns to weigh; one that
    /// starts again as new, or a record that can't be read in time, gets
    /// Core's requests as it stored them, as a record that can't be looked
    /// at is resumed all the same.
    async fn relaunch_values(
        &self,
        adapter: &Arc<dyn Adapter>,
        transport: &Arc<dyn Transport>,
        row: &SessionRow,
        resumed: Option<&str>,
    ) -> settings::Listed {
        let stored = || settings::Listed {
            model: row.model.clone(),
            effort: row.effort.clone(),
            permission_mode: row.permission_mode.clone(),
        };
        let Some(backend_id) = resumed else {
            return stored();
        };
        let reading = adapter.record_settings(
            Arc::clone(transport),
            &row.cwd,
            backend_id,
            settings::requested(row),
        );
        match tokio::time::timeout(RECORD_READ, reading).await {
            Ok(Ok(record)) => settings::relaunched(row, &record),
            _ => stored(),
        }
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
                self.wrote(handle, RowChange::BackendId(id.clone()))?;
            }
            return Ok(HowToStart {
                backend_session_id: chosen,
                resume: None,
                fork_of: None,
                forking: None,
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
                forking: None,
            });
        }
        let Some(original) = row.forked_from.clone() else {
            return Ok(HowToStart {
                backend_session_id: Some(own),
                resume: None,
                fork_of: None,
                forking: None,
            });
        };
        // A fork with no record of its own copies its original again, so it
        // is made as `session.fork` makes one, through the same checks and
        // with the same answer when they refuse (FLOW-RESUME-SESSION-R10).
        // An original Core no longer lists has no checks to pass; the
        // backend then says what is wrong with copying it (FLOW-RESUME-SESSION-R11).
        let forking = match self.session_with_backend_id(&original) {
            Some(original_id) => {
                let marked = self.begin_fork(&original_id).await?;
                let forkable = self
                    .prepare_fork(&original_id, Arc::clone(transport), marked)
                    .await?;
                forkable.beginning.forking
            }
            None => None,
        };
        Ok(HowToStart {
            backend_session_id: Some(own),
            resume: None,
            fork_of: Some(ForkOf {
                original,
                name: lock(&handle.presented).title.clone(),
            }),
            forking,
        })
    }
}

impl SessionManager {
    /// The id of the session whose backend's id is `backend_id`, if Core
    /// lists one.
    fn session_with_backend_id(&self, backend_id: &str) -> Option<String> {
        self.handles()
            .iter()
            .find(|handle| {
                let row = handle.stored_row();
                row.and_then(|row| row.backend_session_id).as_deref() == Some(backend_id)
            })
            .map(|handle| handle.id.clone())
    }
}

/// What looking at a session's folder found: whether it is a folder, or
/// why it could not be looked at.
pub(super) type FolderLook = Result<bool, String>;

/// Refuses with `folder_gone` when the session's folder is not an existing
/// folder (FLOW-RESUME-SESSION-R23). Core looks for itself, before the
/// backend starts. Only the local host has sessions today. One look per
/// session at a time: a look stuck on a drive that stopped answering is
/// joined, never started again beside it ([`read_once`]).
pub(super) async fn check_folder(handle: &Handle) -> Result<(), SessionError> {
    let cwd = handle.row()?.cwd;
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
        Some(Ok(false)) => Err(SessionError::refused(
            ErrorReason::FolderGone,
            format!("the session's folder {cwd} no longer exists, so the session can't be resumed"),
        )),
        // There, perhaps, but not readable: say why rather than call it gone.
        Some(Err(e)) => Err(SessionError::InvalidState(format!(
            "the session's folder {cwd} can't be read: {e}"
        ))),
        None => Err(SessionError::InvalidState(format!(
            "the session's folder {cwd} did not answer within {FOLDER_CHECK:?}"
        ))),
    }
}

/// The one `session.send_progress` a `session.resume` sends, with phase
/// `putting_back`, as it begins to put its session's workspace back or to
/// wait for a put-away or another put-back of it (PROTO-CORE-API-R146).
/// Dropped as the resume answers, it withdraws the line from every
/// connection that has not taken it, so none arrives after the answer.
struct ResumeReport {
    tiles: Arc<TileHub>,
    session: String,
    sent: Arc<Mutex<Reported>>,
}

/// Where the resume's report is: shared with the put-back's own task,
/// which sends it.
enum Reported {
    NotYet,
    Sent(Arc<String>),
    /// The resume has answered, or its request went away: a put-back that
    /// begins to wait only now says nothing.
    Over,
}

impl ResumeReport {
    fn new(tiles: &Arc<TileHub>, session: &str) -> ResumeReport {
        ResumeReport {
            tiles: Arc::clone(tiles),
            session: session.to_string(),
            sent: Arc::new(Mutex::new(Reported::NotYet)),
        }
    }

    /// What the put-back calls once there is something to put back or to
    /// wait for: sends the report, once.
    fn putting_back(&self) -> impl FnOnce() + Send + 'static {
        let tiles = Arc::clone(&self.tiles);
        let session = self.session.clone();
        let sent = Arc::clone(&self.sent);
        move || {
            // Under the lock the withdrawal takes, so the two never cross.
            let mut sent = lock(&sent);
            if !matches!(*sent, Reported::NotYet) {
                return;
            }
            let progress = SendProgressParams {
                session_id: session.clone(),
                phase: SendPhase::PuttingBack,
                copied_bytes: None,
                total_bytes: None,
            };
            let method = ersa_protocol::core_api::notification::SESSION_SEND_PROGRESS;
            let line = notification_line(method, &progress);
            *sent = Reported::Sent(tiles.publish(&session, Latest::SendProgress, line));
        }
    }
}

impl Drop for ResumeReport {
    fn drop(&mut self) {
        let mut sent = lock(&self.sent);
        if let Reported::Sent(line) = std::mem::replace(&mut *sent, Reported::Over) {
            self.tiles
                .withdraw(&self.session, Latest::SendProgress, &line);
        }
    }
}
