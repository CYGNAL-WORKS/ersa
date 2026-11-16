//! `session.fork` (FLOW-FORK-SESSION): when a session can be forked, and
//! what its fork begins with.

use ersa_adapter::ConversationEnd;
use ersa_protocol::core_api::WorkspaceState;

use super::*;

/// What a session begins with, beside what the backend sends it.
#[derive(Default)]
pub struct Beginning {
    /// The title to show until the record gives one.
    pub title: Option<String>,
    /// The conversation it starts from, a fork's. The backend writes a
    /// fork's record only with its first message (fixture
    /// 2.1.316/15-fork-named-no-message), so until then this is the only
    /// place the copied conversation can be read from.
    pub record: Arc<Vec<RecordEvent>>,
    /// For a fork: the operation that marks its original as being forked
    /// (admission's), shared with the fork's reads and its registry check,
    /// and kept until the fork's backend is ready, by when the CLI has read
    /// the record it copies. The mark goes when the last of them lets go.
    pub forking: Option<Arc<UnderWay>>,
}

/// How a fork's title ends.
const FORK_MARK: &str = " (fork)";

/// A fork's title: the original's, shortened so the whole fits its
/// backend's longest title, `max_chars`, then marked (FLOW-FORK-SESSION-R08).
fn fork_title(original: &str, max_chars: usize) -> String {
    let room = max_chars.saturating_sub(FORK_MARK.chars().count());
    let kept: String = original.chars().take(room).collect();
    format!("{}{FORK_MARK}", kept.trim_end())
}

/// A session that can be forked, as [`SessionManager::prepare_fork`] found it.
pub struct Forkable {
    /// The backend's id for the session.
    pub backend_id: String,
    pub adapter: Arc<dyn Adapter>,
    /// The name its fork's backend is to give it (R08), a titled original's.
    pub name: Option<String>,
    /// What its fork begins with.
    pub beginning: Beginning,
}

/// What the reads for a fork need, moved into a task of their own.
struct ForkReads {
    id: String,
    backend: String,
    backend_id: String,
    cwd: String,
    /// How many processes in the registry can be Core's own.
    own: usize,
    /// Whether this is itself a fork, which has no record of its own until
    /// its first message.
    is_fork: bool,
    adapter: Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
}

impl SessionManager {
    /// Refuses a fork of a session a delete has hold of, or whose workspace
    /// one has.
    pub fn refuse_fork_if_busy(&self, id: &str) -> Result<(), SessionError> {
        let handle = self.handle(id)?;
        self.refuse_if_deleting(&handle)?;
        self.refuse_if_claimed(&handle.workspace_id)
    }

    /// Refuses a fork of session `id` unless its conversation, as its record
    /// holds it, ends with a finished reply and nothing else is writing it
    /// (FLOW-FORK-SESSION-R03), each case with its own message.
    ///
    /// The slow reads run in one task, and the session stays marked as being
    /// forked, in admission, until that task ends, even after this request
    /// has given up on it: a disk that hangs then holds one stuck read per
    /// session, not one per try. It stays marked until the fork's backend
    /// is ready too:
    /// `session.send` refuses meanwhile, since a turn begun before the CLI has
    /// read the record would be copied part way (fixture 2.1.316/18), and so
    /// does `session.delete`, whose record the CLI is about to read.
    pub async fn prepare_fork(
        &self,
        id: &str,
        transport: Arc<dyn Transport>,
    ) -> Result<Forkable, SessionError> {
        let handle = self.handle(id)?;
        self.refuse_if_deleting(&handle)?;
        let row = handle.row()?;
        self.refuse_if_claimed(&row.workspace_id)?;
        // A session is forked only from a ready workspace (PROTO-CORE-API,
        // Workspaces).
        let workspace = self.store.workspace(row.workspace_id.clone()).await?;
        if let Some(workspace) = workspace.filter(|w| w.state != WorkspaceState::Ready) {
            return Err(SessionError::InvalidState(format!(
                "workspace {} is not ready",
                workspace.id
            )));
        }
        refuse_if_turn_running(&handle)?;

        let Some(backend_id) = row.backend_session_id else {
            return Err(SessionError::refused(
                ErrorReason::BackendUnnamed,
                "the session's backend has not named it yet, so there is nothing to fork",
            ));
        };
        let adapter = self.adapter_of(&row.backend)?;

        // One step in admission marks the session as being forked, or says
        // why not. From here a send to it and a delete of it are refused,
        // and a send that took the slot first refuses this: one of the two
        // always gives way.
        let command = Command::Fork {
            session: id.to_string(),
        };
        let forking = match self.admission.command(command).await {
            Ok(forking) => forking,
            Err(Refusal::SendInProgress) => return Err(a_turn_is_running(id)),
            Err(Refusal::Forking) => {
                return Err(SessionError::InvalidState(format!(
                    "an earlier fork of session {id} has not finished starting; try again once \
                     it has"
                )));
            }
            Err(refusal) => return Err(refused_session(refusal, &handle.workspace_id, id)),
        };
        // Shared by the reads, the registry check and the fork's start: the
        // mark goes when the last of them lets go.
        let held = Arc::new(forking);
        let for_reads = Arc::clone(&held);
        let reads = ForkReads {
            id: id.to_string(),
            backend: row.backend,
            backend_id: backend_id.clone(),
            cwd: row.cwd,
            own: usize::from(handle.live().is_some()),
            is_fork: row.forked_from.is_some(),
            adapter: Arc::clone(&adapter),
            transport,
        };
        let reading = tokio::spawn(async move { read_for_fork(reads, for_reads).await });
        let record = match tokio::time::timeout(RECORD_READ, reading).await {
            Ok(Ok(record)) => record?,
            Ok(Err(e)) => return Err(SessionError::Internal(format!("the adapter failed: {e}"))),
            Err(_) => {
                return Err(SessionError::InvalidState(format!(
                    "the record of session {id} could not be read within {RECORD_READ:?}"
                )));
            }
        };

        // Those reads can take seconds. The session may have been removed
        // or deleted meanwhile, and a message sent meanwhile starts a turn
        // the fork would copy part of.
        self.refuse_fork_if_busy(id)?;
        let handle = self.handle(id)?;
        refuse_if_turn_running(&handle)?;
        let max_chars = adapter.title_rules().max_chars;
        Ok(Forkable {
            backend_id,
            adapter,
            name: handle
                .session()?
                .title
                .as_deref()
                .map(|title| fork_title(title, max_chars)),
            beginning: Beginning {
                title: None,
                record,
                forking: Some(held),
            },
        })
    }
}

fn refuse_if_turn_running(handle: &Handle) -> Result<(), SessionError> {
    let session = handle.session()?;
    let turn_running = matches!(
        session.state,
        LifecycleState::Running | LifecycleState::WaitingUser
    );
    if (handle.live().is_some() && (turn_running || !session.tile.pending.is_empty()))
        || session.sending.is_some()
    {
        return Err(a_turn_is_running(&handle.id));
    }
    Ok(())
}

/// The refusal of a fork while a turn runs in the session, or a message is
/// on its way to it.
fn a_turn_is_running(session_id: &str) -> SessionError {
    SessionError::refused(
        ErrorReason::SessionBusy,
        format!(
            "a turn is running in session {session_id}; it can be forked once the reply finishes"
        ),
    )
}

/// How the record ends, then who has it open, then the conversation the fork
/// begins with. A record that can't be read can't be said to end with a
/// finished reply, so that too is a refusal (FLOW-FORK-SESSION-R03).
async fn read_for_fork(
    reads: ForkReads,
    held: Arc<UnderWay>,
) -> Result<Arc<Vec<RecordEvent>>, SessionError> {
    let ForkReads {
        id,
        backend,
        backend_id,
        cwd,
        own,
        is_fork,
        adapter,
        transport,
    } = reads;
    let unreadable = |e: AdapterError| {
        SessionError::InvalidState(format!("the record of session {id} could not be read: {e}"))
    };
    let end = adapter
        .conversation_end(Arc::clone(&transport), &cwd, &backend_id)
        .await
        .map_err(unreadable)?;
    // Every unfinished end is the same refusal to the user; the message says
    // which, for the log.
    let unfinished = |message: String| Some((ErrorReason::NoFinishedReply, message));
    let refused = match end {
        Some(ConversationEnd::Finished) => None,
        None => Some((
            ErrorReason::BackendCannotFork,
            format!("the {backend} backend cannot fork a session"),
        )),
        Some(ConversationEnd::NoReply) if is_fork => unfinished(format!(
            "session {id} is a fork with no message of its own yet; it can be forked after its \
             first reply"
        )),
        Some(ConversationEnd::NoReply) => unfinished(format!(
            "no reply has finished in session {id} yet; it can be forked after the first one"
        )),
        Some(ConversationEnd::Interrupted) => unfinished(format!(
            "the last turn in session {id} was interrupted; it can be forked once a reply finishes"
        )),
        Some(ConversationEnd::Unfinished) => unfinished(format!(
            "the record of session {id} ends part way through a turn; it can be forked once a \
             reply finishes"
        )),
    };
    if let Some((reason, message)) = refused {
        return Err(SessionError::refused(reason, message));
    }

    // The registry can't tell Core's own process from another, so it is
    // counted out, and the message can't say which is which; a registry that
    // cannot say does not refuse (PROTO-CORE-API, `session.fork`).
    if let OpenElsewhere::Yes { pids } = ask_open_elsewhere(
        Arc::clone(&adapter),
        Arc::clone(&transport),
        &backend_id,
        Arc::clone(&held),
    )
    .await
        && pids.len() > own
    {
        let whose = if own > 0 {
            "more processes than Ersa's own have"
        } else {
            "another process has"
        };
        return Err(SessionError::open_elsewhere(
            &pids,
            format!(
                "{whose} session {id} open ({}); close it there to fork it",
                pids_named(&pids)
            ),
        ));
    }
    adapter
        .read_record(transport, &cwd, &backend_id)
        .await
        .map_err(unreadable)
}
