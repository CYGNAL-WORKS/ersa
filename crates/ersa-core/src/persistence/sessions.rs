//! Sessions: what finds each one again in its backend, and its mode.
//!
//! A session's history is the backend's own session record (ADR 0017), and
//! so are its title and when it began. Nothing projected is stored here, and
//! neither is the session's state: Core knows that from the backend
//! processes it runs.

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
    /// The workspace's path on the session's host, where the backend runs:
    /// the directory its record is filed under. Read from the workspace.
    pub cwd: String,
}

const SESSION_COLUMNS: &str =
    "s.id, s.workspace_id, s.host_id, s.backend, s.backend_session_id, s.mode, w.path";

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
                     (id, workspace_id, host_id, backend, backend_session_id, mode)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6
                 WHERE EXISTS (SELECT 1 FROM workspaces WHERE id = ?2 AND state = 'ready')",
                    params![
                        session.id,
                        session.workspace_id,
                        session.host_id,
                        session.backend,
                        session.backend_session_id,
                        session.mode,
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

    /// How many sessions a workspace has, running or not.
    pub async fn sessions_in_workspace(&self, workspace_id: String) -> Result<u64> {
        self.call(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sessions WHERE workspace_id = ?1",
                [workspace_id],
                |row| row.get(0),
            )?;
            Ok(count as u64)
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
    })
}
