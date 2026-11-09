//! `session.fork` (FLOW-FORK-SESSION): when a session can be forked, and
//! starting the fork.

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
}

impl SessionManager {
    /// Refuses a fork of session `id` unless its conversation, as its record
    /// holds it, ends with a finished reply and nothing else is writing it
    /// (FLOW-FORK-SESSION-R03), each case with its own message. Returns the
    /// backend's id for the session and its adapter.
    pub async fn check_forkable(
        &self,
        id: &str,
        transport: Arc<dyn Transport>,
    ) -> Result<(String, Arc<dyn Adapter>), SessionError> {
        let handle = self.handle(id)?;
        self.refuse_if_deleting(&handle)?;
        let row = lock(&handle.row).clone();
        self.refuse_if_claimed(&row.workspace_id)?;

        let session = handle.describe();
        let live = handle.live().is_some();
        let turn_running = matches!(
            session.state,
            LifecycleState::Running | LifecycleState::WaitingUser
        );
        if (live && (turn_running || !session.tile.pending.is_empty())) || session.sending.is_some()
        {
            return Err(SessionError::InvalidState(format!(
                "a turn is running in session {id}; it can be forked once the reply finishes"
            )));
        }

        let Some(backend_id) = row.backend_session_id else {
            return Err(SessionError::InvalidState(
                "the session's backend has not named it yet, so there is nothing to fork".into(),
            ));
        };
        let adapter = self.adapter(&row.backend).ok_or_else(|| {
            SessionError::BackendMissing(format!("no adapter for backend {}", row.backend))
        })?;

        let end = {
            let (adapter, transport, cwd, id) = (
                Arc::clone(&adapter),
                Arc::clone(&transport),
                row.cwd.clone(),
                backend_id.clone(),
            );
            bounded(async move { adapter.conversation_end(transport, &cwd, &id).await }).await?
        };
        let refused = match end {
            Some(ConversationEnd::Finished) => None,
            None => Some(format!("the {} backend cannot fork a session", row.backend)),
            Some(ConversationEnd::NoReply) => Some(format!(
                "no reply has finished in session {id} yet; it can be forked after the first one"
            )),
            Some(ConversationEnd::Interrupted) => Some(format!(
                "the last turn in session {id} was interrupted; it can be forked once a reply \
                 finishes"
            )),
            Some(ConversationEnd::Unfinished) => Some(format!(
                "the record of session {id} ends part way through a turn; it can be forked once \
                 a reply finishes"
            )),
        };
        if let Some(refused) = refused {
            return Err(SessionError::InvalidState(refused));
        }

        // The registry can't tell Core's own process from another, so it is
        // counted out; a registry that cannot say does not refuse
        // (PROTO-CORE-API, `session.fork`).
        let own = usize::from(live);
        if let OpenElsewhere::Yes { pids } =
            ask_open_elsewhere(Arc::clone(&adapter), transport, &backend_id).await
            && pids.len() > own
        {
            return Err(SessionError::InvalidState(format!(
                "another process ({}) has session {id} open; close it there to fork it",
                pids_named(&pids)
            )));
        }
        Ok((backend_id, adapter))
    }

    /// Starts `row`, a fork of session `id`, from a copy of its conversation
    /// and, until the backend's record gives one, under its title.
    pub async fn fork(
        &self,
        id: &str,
        row: SessionRow,
        adapter: Arc<dyn Adapter>,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        backend_version: String,
    ) -> Result<Session, SessionError> {
        let handle = self.handle(id)?;
        let original = lock(&handle.row).clone();
        let title = handle.describe().title;
        let record = {
            let (adapter, transport, id) = (
                Arc::clone(&adapter),
                Arc::clone(&transport),
                original.backend_session_id.unwrap_or_default(),
            );
            bounded(async move { adapter.read_record(transport, &original.cwd, &id).await }).await?
        };
        self.create_with(
            row,
            Beginning { title, record },
            adapter,
            transport,
            config,
            backend_version,
        )
        .await
    }
}
