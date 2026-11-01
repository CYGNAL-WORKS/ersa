//! Sessions, their raw logs, and their cached projections.
//!
//! The raw log is append-only and is the truth (ADR 0010); the cache and the
//! summary on the session's row are derived from it. Every write that adds
//! raw lines adds the events they project to in the same transaction, so a
//! crash never leaves the cache behind the log it was derived from.

use ersa_adapter::{Direction, RawLine};
use ersa_protocol::core_api::{Session, Tile};
use ersa_protocol::events::{Envelope, LifecycleState, PROJECTION_VERSION};
use std::sync::Arc;

use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use super::{Error, Result, Store, collect, conflict, from_sql_enum, to_sql_enum};
use crate::raw_log::Projection;

/// What the events so far say about a session, kept on its row.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionProgress {
    pub state: LifecycleState,
    pub tile: Tile,
    pub last_event_seq: Option<u64>,
    pub backend_session_id: Option<String>,
}

impl SessionProgress {
    pub fn of(projection: &Projection) -> Self {
        SessionProgress {
            state: projection.state(),
            tile: projection.tile().clone(),
            last_event_seq: projection.last_seq(),
            backend_session_id: projection.backend_session_id().map(String::from),
        }
    }
}

/// Raw lines and the events they projected to, written together.
pub struct LogBatch {
    pub session_id: String,
    pub lines: Vec<RawLine>,
    pub events: Vec<Envelope>,
    /// The adapter projector's version the events were made with.
    pub projection_version: u32,
    pub progress: SessionProgress,
}

const SESSION_COLUMNS: &str = "id, workspace_id, host_id, backend, mode, title, state, \
                               created_at, last_event_seq, tile";

impl Store {
    /// Records a new session, but only while its workspace is `ready`, in
    /// one statement: a workspace being removed cannot gain a session.
    /// Fails with [`Error::Conflict`] otherwise.
    pub async fn insert_session(&self, session: Session) -> Result<()> {
        self.call(move |conn| {
            let inserted = conn
                .execute(
                    "INSERT INTO sessions
                     (id, workspace_id, host_id, backend, mode, title, state, created_at, tile)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9
                 WHERE EXISTS (SELECT 1 FROM workspaces WHERE id = ?2 AND state = 'ready')",
                    params![
                        session.id,
                        session.workspace_id,
                        session.host_id,
                        session.backend,
                        session.mode,
                        session.title,
                        to_sql_enum(&session.state),
                        session.created_at,
                        serde_json::to_string(&session.tile)?,
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

    pub async fn sessions(&self) -> Result<Vec<Session>> {
        self.call(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {SESSION_COLUMNS} FROM sessions ORDER BY created_at, id"
            ))?;
            let rows = stmt.query_map([], session_row)?;
            collect(rows)
        })
        .await
    }

    pub async fn session(&self, id: String) -> Result<Option<Session>> {
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

    /// Overwrites a session's state, for when its ending could not be
    /// projected. Normally the state follows its events.
    pub async fn set_session_state(&self, id: String, state: LifecycleState) -> Result<()> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE sessions SET state = ?2 WHERE id = ?1",
                params![id, to_sql_enum(&state)],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn set_session_title(&self, id: String, title: String) -> Result<()> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE sessions SET title = ?2 WHERE id = ?1",
                params![id, title],
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

    /// Deletes a session with its raw log and cached projection.
    pub async fn delete_session(&self, id: String) -> Result<()> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            tx.execute("DELETE FROM projection_cache WHERE session_id = ?1", [&id])?;
            tx.execute("DELETE FROM raw_log WHERE session_id = ?1", [&id])?;
            tx.execute("DELETE FROM sessions WHERE id = ?1", [&id])?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// Appends raw lines, caches the events they projected to, and updates
    /// the session's row, all in one transaction.
    ///
    /// Shared rather than owned, so a caller retrying a failed write does not
    /// copy the batch each time.
    pub async fn append_session_log(&self, batch: Arc<LogBatch>) -> Result<()> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            for line in &batch.lines {
                insert_raw_line(&tx, &batch.session_id, line)?;
            }
            for event in &batch.events {
                insert_event(&tx, event, batch.projection_version)?;
            }
            write_progress(&tx, &batch.session_id, &batch.progress)?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// Raw lines `from..=to` of a session: at most `limit` of them, and no
    /// more once they add up to `max_bytes` (always at least one, so a
    /// caller paging through the log makes progress).
    pub async fn raw_log(
        &self,
        session_id: String,
        from: u64,
        to: u64,
        limit: usize,
        max_bytes: usize,
    ) -> Result<Vec<RawLine>> {
        self.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT idx, ts, dir, line FROM raw_log
                 WHERE session_id = ?1 AND idx BETWEEN ?2 AND ?3
                 ORDER BY idx LIMIT ?4",
            )?;
            let rows = stmt.query_map(
                params![session_id, sql_int(from), sql_int(to), limit as i64],
                raw_line_row,
            )?;
            take_bytes(rows, max_bytes, |line| line.line.len())
        })
        .await
    }

    /// The index the session's next raw line gets.
    pub async fn next_raw_index(&self, session_id: String) -> Result<u64> {
        self.call(move |conn| next_index(conn, &session_id)).await
    }

    /// Cached envelopes `from..=to` with their `seq`, made with projector
    /// version `projection_version`, as the JSON they were stored as; no more
    /// once they add up to `max_bytes` (always at least one).
    pub async fn cached_events(
        &self,
        session_id: String,
        projection_version: u32,
        from: u64,
        to: u64,
        max_bytes: usize,
    ) -> Result<Vec<(u64, String)>> {
        self.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT seq, envelope FROM projection_cache
                 WHERE session_id = ?1 AND v = ?2 AND projection_version = ?3
                   AND seq BETWEEN ?4 AND ?5
                 ORDER BY seq",
            )?;
            let rows = stmt.query_map(
                params![
                    session_id,
                    PROJECTION_VERSION,
                    projection_version,
                    sql_int(from),
                    sql_int(to)
                ],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, String>(1)?)),
            )?;
            take_bytes(rows, max_bytes, |(_, envelope)| envelope.len())
        })
        .await
    }

    /// Whether the cached projection is complete and made by this build: only
    /// rows for the current `v` and `projection_version`, numbered without
    /// gaps, and reaching the last raw line.
    pub async fn projection_is_current(
        &self,
        session_id: String,
        projection_version: u32,
    ) -> Result<bool> {
        self.call(move |conn| {
            let stale: i64 = conn.query_row(
                "SELECT COUNT(*) FROM projection_cache
                 WHERE session_id = ?1 AND NOT (v = ?2 AND projection_version = ?3)",
                params![session_id, PROJECTION_VERSION, projection_version],
                |row| row.get(0),
            )?;
            if stale > 0 {
                return Ok(false);
            }
            let (count, max_seq): (i64, Option<i64>) = conn.query_row(
                "SELECT COUNT(*), MAX(seq) FROM projection_cache WHERE session_id = ?1",
                [&session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if max_seq.map_or(0, |max| max + 1) != count {
                return Ok(false);
            }
            let last_envelope: Option<String> = conn
                .query_row(
                    "SELECT envelope FROM projection_cache WHERE session_id = ?1
                     ORDER BY seq DESC LIMIT 1",
                    [&session_id],
                    |row| row.get(0),
                )
                .optional()?;
            let last_raw_to = match last_envelope {
                Some(json) => Some(serde_json::from_str::<Envelope>(&json)?.source.raw_to),
                None => None,
            };
            let last_raw: Option<i64> = conn.query_row(
                "SELECT MAX(idx) FROM raw_log WHERE session_id = ?1",
                [&session_id],
                |row| row.get(0),
            )?;
            Ok(last_raw_to == last_raw.map(|idx| idx as u64))
        })
        .await
    }

    /// Re-derives a session's projection from its raw log with `projection`
    /// (fresh, never fed a line), replacing every cached event and the row's
    /// summary, in one transaction. When `append` is given, that line is
    /// first added to the log, stamped with the next index and `ts`.
    ///
    /// Returns the projection after the whole log, for a caller that carries
    /// on from there.
    pub async fn rebuild_projection(
        &self,
        session_id: String,
        mut projection: Projection,
        append: Option<(Direction, String, String)>,
    ) -> Result<Projection> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            if let Some((dir, ts, line)) = append {
                let index = next_index(&tx, &session_id)?;
                let line = RawLine {
                    index,
                    ts,
                    dir,
                    line,
                };
                insert_raw_line(&tx, &session_id, &line)?;
            }
            tx.execute(
                "DELETE FROM projection_cache WHERE session_id = ?1",
                [&session_id],
            )?;
            let version = projection.projector_version();
            // A page at a time, so a long log is never all in memory.
            const PAGE: i64 = 1000;
            let mut next: i64 = 0;
            loop {
                let lines: Vec<RawLine> = {
                    let mut stmt = tx.prepare_cached(
                        "SELECT idx, ts, dir, line FROM raw_log
                         WHERE session_id = ?1 AND idx >= ?2 ORDER BY idx LIMIT ?3",
                    )?;
                    let rows = stmt.query_map(params![session_id, next, PAGE], raw_line_row)?;
                    collect(rows)?
                };
                let Some(last) = lines.last() else { break };
                next = last.index as i64 + 1;
                for line in &lines {
                    for event in projection.project(line) {
                        insert_event(&tx, &event, version)?;
                    }
                }
            }
            write_progress(&tx, &session_id, &SessionProgress::of(&projection))?;
            tx.commit()?;
            Ok(projection)
        })
        .await
    }
}

fn read_session(conn: &Connection, id: &str) -> Result<Option<Session>> {
    Ok(conn
        .query_row(
            &format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE id = ?1"),
            [id],
            session_row,
        )
        .optional()?)
}

/// One `sessions` row. Read leniently: a tile or state this build cannot
/// read (a corrupt row) shows as an empty tile and `error`, so one bad row
/// cannot make every session unlistable, or keep Core from starting. Both
/// are derived, and re-projection rewrites them.
fn session_row(row: &Row) -> rusqlite::Result<Session> {
    let id: String = row.get(0)?;
    let tile: String = row.get(9)?;
    let tile: Tile = serde_json::from_str(&tile).unwrap_or_else(|e| {
        tracing::error!(session = %id, error = %e, "unreadable tile; showing an empty one");
        Tile::default()
    });
    let state = from_sql_enum(row, 6).unwrap_or_else(|e| {
        tracing::error!(session = %id, error = %e, "unreadable state; showing error");
        LifecycleState::Error
    });
    Ok(Session {
        id,
        workspace_id: row.get(1)?,
        host_id: row.get(2)?,
        backend: row.get(3)?,
        mode: row.get(4)?,
        title: row.get(5)?,
        state,
        created_at: row.get(7)?,
        last_event_seq: row.get::<_, Option<i64>>(8)?.map(|seq| seq as u64),
        tile,
    })
}

fn raw_line_row(row: &Row) -> rusqlite::Result<RawLine> {
    Ok(RawLine {
        index: row.get::<_, i64>(0)? as u64,
        ts: row.get(1)?,
        dir: from_sql_enum(row, 2)?,
        line: row.get(3)?,
    })
}

fn insert_raw_line(tx: &Transaction, session_id: &str, line: &RawLine) -> Result<()> {
    tx.prepare_cached(
        "INSERT INTO raw_log (session_id, idx, ts, dir, line) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?
    .execute(params![
        session_id,
        line.index as i64,
        line.ts,
        to_sql_enum(&line.dir),
        line.line
    ])?;
    Ok(())
}

fn insert_event(tx: &Transaction, event: &Envelope, projection_version: u32) -> Result<()> {
    tx.prepare_cached(
        "INSERT INTO projection_cache (session_id, v, projection_version, seq, envelope)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?
    .execute(params![
        event.session_id,
        event.v,
        projection_version,
        event.seq as i64,
        serde_json::to_string(event)?
    ])?;
    Ok(())
}

fn write_progress(tx: &Transaction, session_id: &str, progress: &SessionProgress) -> Result<()> {
    tx.execute(
        "UPDATE sessions
         SET state = ?2, tile = ?3, last_event_seq = ?4,
             backend_session_id = COALESCE(?5, backend_session_id)
         WHERE id = ?1",
        params![
            session_id,
            to_sql_enum(&progress.state),
            serde_json::to_string(&progress.tile)?,
            progress.last_event_seq.map(|seq| seq as i64),
            progress.backend_session_id
        ],
    )?;
    Ok(())
}

fn next_index(conn: &Connection, session_id: &str) -> Result<u64> {
    let last: Option<i64> = conn.query_row(
        "SELECT MAX(idx) FROM raw_log WHERE session_id = ?1",
        [session_id],
        |row| row.get(0),
    )?;
    Ok(last.map_or(0, |idx| idx as u64 + 1))
}

/// Rows until they add up to `max_bytes` by `size`, and always the first:
/// lines can be large, so a count alone does not bound what is read.
fn take_bytes<T>(
    rows: impl Iterator<Item = rusqlite::Result<T>>,
    max_bytes: usize,
    size: impl Fn(&T) -> usize,
) -> Result<Vec<T>> {
    let mut out = Vec::new();
    let mut total = 0;
    for row in rows {
        let row = row?;
        total += size(&row);
        if total > max_bytes && !out.is_empty() {
            break;
        }
        out.push(row);
    }
    Ok(out)
}

/// A `u64` bound as SQLite's signed integer, saturating: an index past
/// `i64::MAX` means "everything".
fn sql_int(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
