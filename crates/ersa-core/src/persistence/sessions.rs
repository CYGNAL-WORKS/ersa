//! Sessions: what finds each one again in its backend, its mode, and the
//! user's arrangement of it (ADR 0026).
//!
//! A session's history is the backend's own session record (ADR 0017), and
//! so are its title and when it began. Nothing projected is stored here, and
//! neither is the session's state: Core knows that from the backend
//! processes it runs.

use ersa_protocol::core_api::Deleting;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{Error, Result, Store, conflict};

/// One session's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub id: String,
    pub workspace_id: String,
    pub host_id: String,
    pub backend: String,
    /// The backend's own id for the session, which finds its record; `None`
    /// until the backend has announced it.
    pub backend_session_id: Option<String>,
    /// Ersa's mode for the session (milestone M3).
    pub mode: Option<String>,
    /// Core's latest request for the session's model, effort and permission
    /// mode, ids from `backend.list`: what it last gave the backend at
    /// `session.create` or `session.fork`. `None` for a model or effort left
    /// to its backend, and for every session created before Core recorded
    /// them (PROTO-CORE-API-R71).
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_mode: Option<String>,
    /// When Core gave the backend each of those, RFC 3339 on the host's
    /// clock (PROTO-CORE-API-R71); `None` for a value it never gave, and for
    /// a session from before Core kept the time.
    pub model_at: Option<String>,
    pub effort_at: Option<String>,
    pub permission_mode_at: Option<String>,
    /// The workspace's path on the session's host, where the backend runs:
    /// the directory its record is filed under. Read from the workspace.
    pub cwd: String,
    /// Whether the user archived the session (ADR 0026). A new session is
    /// not archived, so `insert_session` leaves it to the column's default.
    pub archived: bool,
    /// Where the user's delete of the session is, when it is marked
    /// deleting (FLOW-ARCHIVE-DELETE-SESSION). A new session is not, so
    /// `insert_session` leaves it to the column's default.
    pub deleting: Option<Deleting>,
    /// Whether the user pinned the session, and the section they put it in
    /// (ADR 0026). A new session is not pinned, so `insert_session` leaves that
    /// to the column's default; a fork starts in its original's section.
    pub pinned: bool,
    pub section_id: Option<String>,
    /// RFC 3339: when the session's latest turn ended, `None` until one has
    /// (PROTO-CORE-API-R37).
    pub last_activity_at: Option<String>,
    /// For a fork: the backend's id for the session it copies, whose record
    /// holds the fork's conversation until the backend writes one of its own,
    /// and the last message of that conversation.
    pub forked_from: Option<String>,
    pub forked_through: Option<String>,
}

const SESSION_COLUMNS: &str = "s.id, s.workspace_id, s.host_id, s.backend, s.backend_session_id, \
     s.mode, w.path, s.model, s.effort, s.permission_mode, s.archived, s.deleting, \
     s.pinned, s.section_id, s.last_activity_at, s.forked_from, s.forked_through,      s.model_at, s.effort_at, s.permission_mode_at";

impl Store {
    /// Records a new session, but only while its workspace is `ready`, in
    /// one statement: a workspace being removed cannot gain a session.
    /// Fails with [`Error::Conflict`] otherwise. `cwd` is not written; it is
    /// the workspace's.
    pub async fn insert_session(&self, session: SessionRow) -> Result<()> {
        self.call(move |conn| {
            let inserted = conn
                .execute(
                    "INSERT INTO sessions
                     (id, workspace_id, host_id, backend, backend_session_id, mode,
                      model, effort, permission_mode, section_id, forked_from, forked_through,
                      model_at, effort_at, permission_mode_at)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15
                 WHERE EXISTS (SELECT 1 FROM workspaces WHERE id = ?2 AND state = 'ready')",
                    params![
                        session.id,
                        session.workspace_id,
                        session.host_id,
                        session.backend,
                        session.backend_session_id,
                        session.mode,
                        session.model,
                        session.effort,
                        session.permission_mode,
                        session.section_id,
                        session.forked_from,
                        session.forked_through,
                        session.model_at,
                        session.effort_at,
                        session.permission_mode_at,
                    ],
                )
                .map_err(|e| conflict(e, "the workspace changed meanwhile"))?;
            if inserted == 0 {
                return Err(Error::Conflict(format!(
                    "workspace {} is not ready",
                    session.workspace_id
                )));
            }
            Ok(())
        })
        .await
    }

    pub async fn sessions(&self) -> Result<Vec<SessionRow>> {
        self.call(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {SESSION_COLUMNS} FROM sessions s JOIN workspaces w ON w.id = s.workspace_id"
            ))?;
            let rows = stmt.query_map([], session_row)?;
            super::collect(rows)
        })
        .await
    }

    pub async fn session(&self, id: String) -> Result<Option<SessionRow>> {
        self.call(move |conn| read_session(conn, &id)).await
    }

    /// The ids of a workspace's sessions, running or not, sorted.
    pub async fn sessions_in_workspace(&self, workspace_id: String) -> Result<Vec<String>> {
        self.call(move |conn| {
            let mut statement =
                conn.prepare("SELECT id FROM sessions WHERE workspace_id = ?1 ORDER BY id")?;
            let ids = statement
                .query_map([workspace_id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(ids)
        })
        .await
    }

    /// Records the backend's own id for the session, once it announced it.
    pub async fn set_backend_session_id(
        &self,
        id: String,
        backend_session_id: String,
    ) -> Result<()> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE sessions SET backend_session_id = ?2 WHERE id = ?1",
                params![id, backend_session_id],
            )?;
            Ok(())
        })
        .await
    }

    /// Forgets which session a fork was copied from, once it has a record of
    /// its own (FLOW-FORK-SESSION-R09).
    pub async fn clear_fork_link(&self, id: String) -> Result<()> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE sessions SET forked_from = NULL, forked_through = NULL WHERE id = ?1",
                params![id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn set_session_mode(&self, id: String, mode: String) -> Result<()> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE sessions SET mode = ?2 WHERE id = ?1",
                params![id, mode],
            )?;
            Ok(())
        })
        .await
    }

    /// Marks the session archived, or not (ADR 0026).
    pub async fn set_session_archived(&self, id: String, archived: bool) -> Result<()> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE sessions SET archived = ?2 WHERE id = ?1",
                params![id, archived],
            )?;
            Ok(())
        })
        .await
    }

    /// Changes the arrangement fields given (ADR 0026): `pinned` when
    /// `Some`, and the section when `Some`, `Some(None)` being no section.
    /// Returns `false`, changing nothing, when the section named does not
    /// exist.
    pub async fn arrange_session(
        &self,
        id: String,
        pinned: Option<bool>,
        section_id: Option<Option<String>>,
    ) -> Result<bool> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            if let Some(Some(section_id)) = &section_id {
                let exists = tx
                    .query_row("SELECT 1 FROM sections WHERE id = ?1", [section_id], |_| {
                        Ok(())
                    })
                    .optional()?
                    .is_some();
                if !exists {
                    return Ok(false);
                }
            }
            // `COALESCE` keeps the pin when none is given; the `CASE` keeps
            // the section unless one (or none, as NULL) is given.
            tx.execute(
                "UPDATE sessions SET
                     pinned = COALESCE(?2, pinned),
                     section_id = CASE WHEN ?3 THEN ?4 ELSE section_id END
                 WHERE id = ?1",
                params![id, pinned, section_id.is_some(), section_id.flatten()],
            )?;
            tx.commit()?;
            Ok(true)
        })
        .await
    }

    /// Records when the session's latest turn ended (PROTO-CORE-API-R37).
    pub async fn set_session_last_activity(&self, id: String, at: String) -> Result<()> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE sessions SET last_activity_at = ?2 WHERE id = ?1",
                params![id, at],
            )?;
            Ok(())
        })
        .await
    }

    /// The ids of a workspace's sessions that are not marked deleting,
    /// sorted: those `workspace.changes` counts.
    pub async fn sessions_counted_in_workspace(&self, workspace_id: String) -> Result<Vec<String>> {
        self.call(move |conn| {
            let mut statement = conn.prepare(
                "SELECT id FROM sessions WHERE workspace_id = ?1 AND deleting IS NULL ORDER BY id",
            )?;
            let ids = statement
                .query_map([workspace_id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(ids)
        })
        .await
    }

    /// Marks the session deleting, `running`, and records whether the delete
    /// takes the workspace's files: `files` the first time, and after that
    /// only ever narrowed, so files once to stay still stay. When the record
    /// says they go, the workspace's state becomes `error` in the same
    /// transaction, so no session can start in it. Returns what is recorded.
    pub async fn mark_session_deleting(&self, id: String, files: bool) -> Result<bool> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "UPDATE sessions SET deleting = 'running',
                     delete_files = CASE WHEN delete_files IS NULL THEN ?2
                                         ELSE delete_files AND ?2 END
                 WHERE id = ?1",
                params![id, files],
            )?;
            let recorded: bool = tx.query_row(
                "SELECT delete_files FROM sessions WHERE id = ?1",
                [&id],
                |row| row.get(0),
            )?;
            if recorded {
                tx.execute(
                    "UPDATE workspaces SET state = 'error'
                     WHERE id = (SELECT workspace_id FROM sessions WHERE id = ?1)",
                    [&id],
                )?;
            }
            tx.commit()?;
            Ok(recorded)
        })
        .await
    }

    /// Marks the session's delete failed.
    pub async fn set_session_delete_failed(&self, id: String) -> Result<()> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE sessions SET deleting = 'failed' WHERE id = ?1 AND deleting IS NOT NULL",
                [id],
            )?;
            Ok(())
        })
        .await
    }

    /// Marks every delete a previous Core left `running` as `failed`, and
    /// returns how many there were. Run at startup, before any request, when
    /// no delete can still be in flight: nothing more is deleted until the
    /// user tries again.
    pub async fn fail_running_deletes(&self) -> Result<usize> {
        self.call(|conn| {
            Ok(conn.execute(
                "UPDATE sessions SET deleting = 'failed' WHERE deleting = 'running'",
                [],
            )?)
        })
        .await
    }

    /// Removes a deleted session, and its workspace with it when
    /// `workspace_id` names one and no other session refers to it (one
    /// marked deleting, whose own delete keeps the files, may), in one
    /// transaction.
    pub async fn finish_session_delete(
        &self,
        id: String,
        workspace_id: Option<String>,
    ) -> Result<()> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            tx.execute("DELETE FROM sessions WHERE id = ?1", [&id])?;
            if let Some(workspace_id) = workspace_id {
                tx.execute(
                    "DELETE FROM workspaces WHERE id = ?1
                     AND NOT EXISTS (SELECT 1 FROM sessions WHERE workspace_id = ?1)",
                    [workspace_id],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// Forgets a session. Its record is the backend's, and stays.
    pub async fn delete_session(&self, id: String) -> Result<()> {
        self.call(move |conn| {
            conn.execute("DELETE FROM sessions WHERE id = ?1", [&id])?;
            Ok(())
        })
        .await
    }
}

fn read_session(conn: &Connection, id: &str) -> Result<Option<SessionRow>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {SESSION_COLUMNS} FROM sessions s JOIN workspaces w ON w.id = s.workspace_id
                 WHERE s.id = ?1"
            ),
            [id],
            session_row,
        )
        .optional()?)
}

fn session_row(row: &Row) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        host_id: row.get(2)?,
        backend: row.get(3)?,
        backend_session_id: row.get(4)?,
        mode: row.get(5)?,
        cwd: row.get(6)?,
        model: row.get(7)?,
        effort: row.get(8)?,
        permission_mode: row.get(9)?,
        // SQLite has no boolean type: the column holds 0 or 1, which
        // rusqlite reads as a `bool`.
        archived: row.get(10)?,
        deleting: match row.get::<_, Option<String>>(11)?.as_deref() {
            None => None,
            Some("running") => Some(Deleting::Running),
            // The column's check allows only the two.
            Some(_) => Some(Deleting::Failed),
        },
        pinned: row.get(12)?,
        section_id: row.get(13)?,
        last_activity_at: row.get(14)?,
        forked_from: row.get(15)?,
        forked_through: row.get(16)?,
        model_at: row.get(17)?,
        effort_at: row.get(18)?,
        permission_mode_at: row.get(19)?,
    })
}
