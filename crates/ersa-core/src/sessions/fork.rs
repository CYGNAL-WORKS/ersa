//! `session.fork` (FLOW-FORK-SESSION): when a session can be forked, and
//! what its fork begins with.

use ersa_adapter::ConversationEnd;

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
    /// For a fork: what is left to do once its backend is ready.
    pub fork: Option<ForkStart>,
}

/// What a fork's start still has to do.
pub struct ForkStart {
    /// Keeps the original's `forking` flag raised until the fork's backend
    /// is ready, by when the CLI has read the record it copies.
    pub hold: Arc<ForkHold>,
    /// The name to give it in the backend, a titled original's (R06). The
    /// backend keeps it, as it keeps any name a session is given.
    pub name: Option<String>,
}

/// How a fork's title ends.
const FORK_MARK: &str = " (fork)";

/// A fork's title: the original's, shortened so the whole fits the limit on a
/// title, then marked (FLOW-FORK-SESSION-R06).
fn fork_title(original: &str) -> String {
    let room = MAX_TITLE_CHARS - FORK_MARK.chars().count();
    let kept: String = original.chars().take(room).collect();
    format!("{}{FORK_MARK}", kept.trim_end())
}

/// A session that can be forked, as [`SessionManager::prepare_fork`] found it.
pub struct Forkable {
    /// The backend's id for the session.
    pub backend_id: String,
    pub adapter: Arc<dyn Adapter>,
    /// What its fork begins with.
    pub beginning: Beginning,
}

/// Lowers a session's `forking` flag when the last holder lets go.
pub struct ForkHold(Arc<AtomicBool>);

impl Drop for ForkHold {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
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
        let workspace_id = lock(&handle.row).workspace_id.clone();
        self.refuse_if_claimed(&workspace_id)
    }

    /// Refuses a fork of session `id` unless its conversation, as its record
    /// holds it, ends with a finished reply and nothing else is writing it
    /// (FLOW-FORK-SESSION-R03), each case with its own message.
    ///
    /// The slow reads run in one task, and the session's `forking` flag stays
    /// raised until that task ends, even after this request has given up on
    /// it: a disk that hangs then holds one stuck read per session, not one
    /// per try. It stays raised until the fork's backend is ready too:
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
        let row = lock(&handle.row).clone();
        self.refuse_if_claimed(&row.workspace_id)?;
        refuse_if_turn_running(&handle)?;

        let Some(backend_id) = row.backend_session_id else {
            return Err(SessionError::InvalidState(
                "the session's backend has not named it yet, so there is nothing to fork".into(),
            ));
        };
        let adapter = self.adapter(&row.backend).ok_or_else(|| {
            SessionError::BackendMissing(format!("no adapter for backend {}", row.backend))
        })?;

        if handle.forking.swap(true, Ordering::SeqCst) {
            return Err(SessionError::InvalidState(format!(
                "an earlier fork of session {id} has not finished starting; try again once it \
                 has"
            )));
        }
        let held = Arc::new(ForkHold(Arc::clone(&handle.forking)));
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
        let title = handle.describe().title.as_deref().map(fork_title);
        Ok(Forkable {
            backend_id,
            adapter,
            beginning: Beginning {
                fork: Some(ForkStart {
                    hold: held,
                    name: title.clone(),
                }),
                title,
                record,
            },
        })
    }
}

fn refuse_if_turn_running(handle: &Handle) -> Result<(), SessionError> {
    let session = handle.describe();
    let turn_running = matches!(
        session.state,
        LifecycleState::Running | LifecycleState::WaitingUser
    );
    if (handle.live().is_some() && (turn_running || !session.tile.pending.is_empty()))
        || session.sending.is_some()
    {
        return Err(SessionError::InvalidState(format!(
            "a turn is running in session {}; it can be forked once the reply finishes",
            handle.id
        )));
    }
    Ok(())
}

/// How the record ends, then who has it open, then the conversation the fork
/// begins with. A record that can't be read can't be said to end with a
/// finished reply, so that too is a refusal (FLOW-FORK-SESSION-R03).
async fn read_for_fork(
    reads: ForkReads,
    held: Arc<ForkHold>,
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
    let refused = match end {
        Some(ConversationEnd::Finished) => None,
        None => Some(format!("the {backend} backend cannot fork a session")),
        Some(ConversationEnd::NoReply) if is_fork => Some(format!(
            "session {id} is a fork with no message of its own yet; it can be forked after its \
             first reply"
        )),
        Some(ConversationEnd::NoReply) => Some(format!(
            "no reply has finished in session {id} yet; it can be forked after the first one"
        )),
        Some(ConversationEnd::Interrupted) => Some(format!(
            "the last turn in session {id} was interrupted; it can be forked once a reply finishes"
        )),
        Some(ConversationEnd::Unfinished) => Some(format!(
            "the record of session {id} ends part way through a turn; it can be forked once a \
             reply finishes"
        )),
    };
    if let Some(refused) = refused {
        return Err(SessionError::InvalidState(refused));
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
        return Err(SessionError::InvalidState(format!(
            "{whose} session {id} open ({}); close it there to fork it",
            pids_named(&pids)
        )));
    }
    adapter
        .read_record(transport, &cwd, &backend_id)
        .await
        .map_err(unreadable)
}
