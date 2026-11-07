//! Persistence: one local database per Core process holding projects, hosts,
//! workspaces, settings, and per session only what finds and presents it
//! (`ARCH-CORE`, Persistence). A session's history is the backend's own
//! session record and is never copied here (ADR 0017). SQLite in the user's application data
//! directory, with schema migrations run at startup.
//!
//! [`Store`] owns the connection. Its methods are `async` and run SQLite on
//! Tokio's blocking thread pool, so a slow disk never stalls the tasks serving
//! the socket. Each method is one short statement or transaction; the
//! connection sits behind a mutex, so they run one at a time.
//!
//! Rows come back as the `ersa-protocol` Core API types where a table maps
//! one-to-one onto a wire object (projects, workspaces), which keeps a single
//! definition of each.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ersa_protocol::core_api::{HostKind, Project, Vcs, Workspace, WorkspaceKind, WorkspaceState};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::settings;

mod sessions;

pub use sessions::SessionRow;

/// The migrations, in order. Migration `n` (1-based) is applied when the
/// database's `user_version` is below `n`, and then sets it to `n`. Append
/// new migrations; never edit one that has shipped.
const MIGRATIONS: &[&str] = &[
    include_str!("persistence/0001_initial.sql"),
    include_str!("persistence/0002_session_tiles.sql"),
    include_str!("persistence/0003_session_record.sql"),
    include_str!("persistence/0004_project_trust.sql"),
    include_str!("persistence/0005_root_workspace.sql"),
    include_str!("persistence/0006_session_archived.sql"),
    include_str!("persistence/0007_session_deleting.sql"),
];

/// The id of the local host's row, which always exists.
pub const LOCAL_HOST_ID: &str = "local";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("stored JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("the database file is from a newer Ersa (schema {found}, this build knows {known})")]
    TooNew { found: usize, known: usize },
    #[error("a database task panicked")]
    TaskPanicked,
    /// A settings patch would store a value of the wrong type.
    #[error("invalid settings: {0}")]
    InvalidSettings(String),
    /// A uniqueness or foreign-key constraint refused the change: another
    /// request changed the same rows first.
    #[error("{0}")]
    Conflict(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// The database. Cheap to clone: clones share one connection.
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

/// A row of the `hosts` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRow {
    pub id: String,
    pub name: String,
    pub kind: HostKind,
}

impl Store {
    /// Opens (or creates) the database at `path` and brings its schema up to
    /// date. Blocking; called once at startup.
    pub fn open(path: &Path) -> Result<Store> {
        let mut conn = Connection::open(path)?;
        // WAL lets readers proceed while a write is in progress; foreign keys
        // are off by default in SQLite and must be turned on per connection.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&mut conn, path)?;
        Ok(Store {
            conn: Arc::new(Mutex::new(conn)),
            path: path.to_path_buf(),
        })
    }

    /// Where the database file is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Runs `f` with the connection on the blocking thread pool.
    ///
    /// The building block for every method below, and for code elsewhere in
    /// Core (the session manager) that needs a query of its own.
    pub async fn call<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            // A panic while holding the lock poisons it; the connection itself
            // is still usable, since SQLite rolls back unfinished transactions.
            let mut conn = conn.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut conn)
        })
        .await
        .map_err(|_| Error::TaskPanicked)?
    }

    // -- Hosts -------------------------------------------------------------

    /// Creates the local host's row if it is missing, and keeps its name
    /// current (the machine may have been renamed).
    pub async fn ensure_local_host(&self, name: String) -> Result<()> {
        self.call(move |conn| {
            conn.execute(
                "INSERT INTO hosts (id, name, kind, created_at) VALUES (?1, ?2, 'local', ?3)
                 ON CONFLICT (id) DO UPDATE SET name = excluded.name",
                params![LOCAL_HOST_ID, name, now_rfc3339()],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn hosts(&self) -> Result<Vec<HostRow>> {
        self.call(|conn| {
            let mut stmt =
                conn.prepare("SELECT id, name, kind FROM hosts ORDER BY created_at, id")?;
            let rows = stmt.query_map([], host_row)?;
            collect(rows)
        })
        .await
    }

    pub async fn host(&self, id: String) -> Result<Option<HostRow>> {
        self.call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT id, name, kind FROM hosts WHERE id = ?1",
                    [id],
                    host_row,
                )
                .optional()?)
        })
        .await
    }

    // -- Projects ----------------------------------------------------------

    pub async fn projects(&self) -> Result<Vec<Project>> {
        self.call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, host_id, path, name, vcs, trusted FROM projects ORDER BY created_at, id",
            )?;
            let rows = stmt.query_map([], project_row)?;
            collect(rows)
        })
        .await
    }

    pub async fn project(&self, id: String) -> Result<Option<Project>> {
        self.call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT id, host_id, path, name, vcs, trusted FROM projects WHERE id = ?1",
                    [id],
                    project_row,
                )
                .optional()?)
        })
        .await
    }

    /// The project already added on `host_id` whose path has `path_key`.
    pub async fn project_at(&self, host_id: String, path_key: String) -> Result<Option<Project>> {
        self.call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT id, host_id, path, name, vcs, trusted FROM projects
                     WHERE host_id = ?1 AND path_key = ?2",
                    [host_id, path_key],
                    project_row,
                )
                .optional()?)
        })
        .await
    }

    /// Adds a project and its root workspace, the project's own folder, in
    /// one transaction, and returns that workspace. `path_key` is its path as
    /// the host compares paths. Fails with [`Error::Conflict`] when that path
    /// was added meanwhile.
    pub async fn insert_project(&self, project: Project, path_key: String) -> Result<Workspace> {
        self.call(move |conn| {
            let root = Workspace {
                id: new_id("w"),
                project_id: project.id.clone(),
                kind: WorkspaceKind::Root,
                path: project.path.clone(),
                // The root's checkout is the user's to change; Core names none.
                vcs_ref: String::new(),
                state: WorkspaceState::Ready,
            };
            let created_at = now_rfc3339();
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO projects (id, host_id, path, path_key, name, vcs, trusted, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    project.id,
                    project.host_id,
                    project.path,
                    path_key,
                    project.name,
                    to_sql_enum(&project.vcs),
                    project.trusted,
                    created_at
                ],
            )
            .map_err(|e| conflict(e, "that folder was added as a project meanwhile"))?;
            insert_workspace(&tx, &root, &created_at)?;
            tx.commit()?;
            Ok(root)
        })
        .await
    }

    /// Records whether the user trusts project `id`. Returns the project as
    /// it now is, or `None` when there is no such project.
    pub async fn set_project_trust(&self, id: String, trusted: bool) -> Result<Option<Project>> {
        self.call(move |conn| {
            Ok(conn
                .query_row(
                    "UPDATE projects SET trusted = ?2 WHERE id = ?1
                     RETURNING id, host_id, path, name, vcs, trusted",
                    params![id, trusted],
                    project_row,
                )
                .optional()?)
        })
        .await
    }

    /// Removes a project and its root workspace, in one transaction. Fails
    /// with [`Error::Conflict`] when an isolated workspace was created in it,
    /// or a session in its root workspace, meanwhile: the foreign keys refuse
    /// the deletes, and nothing is removed.
    pub async fn delete_project(&self, id: String) -> Result<()> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "DELETE FROM workspaces WHERE project_id = ?1 AND kind = 'root'",
                [&id],
            )
            .map_err(|e| conflict(e, "a session was started in the project meanwhile"))?;
            tx.execute("DELETE FROM projects WHERE id = ?1", [&id])
                .map_err(|e| conflict(e, "a workspace was created in the project meanwhile"))?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    // -- Workspaces --------------------------------------------------------

    /// A project's workspaces, its root workspace first.
    pub async fn workspaces(&self, project_id: String) -> Result<Vec<Workspace>> {
        self.call(move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {WORKSPACE_COLUMNS} FROM workspaces
                 WHERE project_id = ?1 ORDER BY kind = 'root' DESC, created_at, id"
            ))?;
            let rows = stmt.query_map([project_id], workspace_row)?;
            collect(rows)
        })
        .await
    }

    pub async fn workspace(&self, id: String) -> Result<Option<Workspace>> {
        self.call(move |conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {WORKSPACE_COLUMNS} FROM workspaces WHERE id = ?1"),
                    [id],
                    workspace_row,
                )
                .optional()?)
        })
        .await
    }

    /// Records a workspace. Fails with [`Error::Conflict`] when its project
    /// was removed meanwhile.
    pub async fn insert_workspace(&self, workspace: Workspace) -> Result<()> {
        self.call(move |conn| insert_workspace(conn, &workspace, &now_rfc3339()))
            .await
    }

    pub async fn set_workspace_state(&self, id: String, state: WorkspaceState) -> Result<()> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE workspaces SET state = ?2 WHERE id = ?1",
                params![id, to_sql_enum(&state)],
            )?;
            Ok(())
        })
        .await
    }

    /// Moves a workspace from `from` to `to` only if it is currently in one of
    /// the `from` states, as one statement, so two removals racing on
    /// different connections cannot both start. Returns whether it moved.
    pub async fn transition_workspace(
        &self,
        id: String,
        from: &'static [WorkspaceState],
        to: WorkspaceState,
    ) -> Result<bool> {
        self.call(move |conn| {
            let from: Vec<String> = from.iter().map(to_sql_enum).collect();
            // `json_each` turns a JSON array into rows, which lets one bound
            // parameter stand for the whole list.
            let changed = conn.execute(
                "UPDATE workspaces SET state = ?2
                 WHERE id = ?1 AND state IN (SELECT value FROM json_each(?3))",
                params![id, to_sql_enum(&to), serde_json::to_string(&from)?],
            )?;
            Ok(changed == 1)
        })
        .await
    }

    /// Marks every workspace a previous Core left `creating` or `removing` as
    /// `error`, and returns how many there were. Run at startup, before any
    /// request, when no operation can still be in flight.
    pub async fn reconcile_workspaces(&self) -> Result<usize> {
        self.call(|conn| {
            Ok(conn.execute(
                "UPDATE workspaces SET state = 'error' WHERE state IN ('creating', 'removing')",
                [],
            )?)
        })
        .await
    }

    /// Deletes a workspace's row. Fails with [`Error::Conflict`] when a
    /// session refers to it.
    pub async fn delete_workspace(&self, id: String) -> Result<()> {
        self.call(move |conn| {
            conn.execute("DELETE FROM workspaces WHERE id = ?1", [id])
                .map_err(|e| conflict(e, "a session was created in the workspace meanwhile"))?;
            Ok(())
        })
        .await
    }

    // -- Settings ----------------------------------------------------------

    /// The user settings as one JSON object.
    pub async fn settings(&self) -> Result<Map<String, Value>> {
        self.call(|conn| read_settings(conn)).await
    }

    /// Merges `patch` into the settings (RFC 7386) and returns the result.
    /// One transaction, so concurrent patches never lose each other's keys.
    pub async fn patch_settings(&self, patch: Map<String, Value>) -> Result<Map<String, Value>> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let before = read_settings(&tx)?;
            let mut after = Value::Object(before.clone());
            settings::merge_patch(&mut after, &Value::Object(patch));
            let Value::Object(after) = after else {
                unreachable!("merging an object patch into an object yields an object")
            };
            settings::validate(&after).map_err(Error::InvalidSettings)?;
            for key in before.keys().filter(|k| !after.contains_key(*k)) {
                tx.execute("DELETE FROM settings WHERE key = ?1", [key])?;
            }
            for (key, value) in &after {
                tx.execute(
                    "INSERT INTO settings (key, value) VALUES (?1, ?2)
                     ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                    params![key, serde_json::to_string(value)?],
                )?;
            }
            tx.commit()?;
            Ok(after)
        })
        .await
    }
}

/// The migration that drops data no later Core can rebuild (the raw log, and
/// each session's stored title and creation time): a copy of the database
/// is kept before it runs. The space it frees is not reclaimed: SQLite
/// reuses free pages as the database grows, and shrinking the file is not
/// worth delaying Core's start.
const DESTRUCTIVE_MIGRATION: usize = 3;

/// Applies every migration the database at `path` has not seen yet, each in
/// its own transaction together with the `user_version` bump.
fn migrate(conn: &mut Connection, path: &Path) -> Result<()> {
    let current = user_version(conn)?;
    if current > MIGRATIONS.len() {
        return Err(Error::TooNew {
            found: current,
            known: MIGRATIONS.len(),
        });
    }
    let destructive = (1..DESTRUCTIVE_MIGRATION).contains(&current);
    if destructive {
        // A consistent copy, taken through the connection rather than of the
        // file (the WAL may hold what the file does not yet).
        let backup = backup_path(path, current);
        let _ = std::fs::remove_file(&backup);
        conn.execute("VACUUM INTO ?1", [backup.to_string_lossy()])?;
        tracing::info!(backup = %backup.display(), "kept a copy of the database before migrating it");
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (index + 1) as i64)?;
        tx.commit()?;
        tracing::info!(migration = index + 1, "applied database migration");
    }
    Ok(())
}

/// Where the copy of the database at `path`, at schema `version`, is kept
/// before a migration that drops data: beside it, named for the version.
pub fn backup_path(path: &Path, version: usize) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".v{version}.bak"));
    path.with_file_name(name)
}

/// The schema version recorded in the database file; 0 for a new file.
fn user_version(conn: &Connection) -> rusqlite::Result<usize> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    Ok(version.max(0) as usize)
}

fn read_settings(conn: &Connection) -> Result<Map<String, Value>> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
    let mut rows = stmt.query([])?;
    let mut map = Map::new();
    while let Some(row) = rows.next()? {
        let key: String = row.get(0)?;
        let value: String = row.get(1)?;
        map.insert(key, serde_json::from_str(&value)?);
    }
    Ok(map)
}

fn host_row(row: &Row) -> rusqlite::Result<HostRow> {
    Ok(HostRow {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: from_sql_enum(row, 2)?,
    })
}

fn project_row(row: &Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        host_id: row.get(1)?,
        path: row.get(2)?,
        name: row.get(3)?,
        vcs: from_sql_enum::<Vcs>(row, 4)?,
        trusted: row.get(5)?,
    })
}

const WORKSPACE_COLUMNS: &str = "id, project_id, path, vcs_ref, state, kind";

fn workspace_row(row: &Row) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: row.get(0)?,
        project_id: row.get(1)?,
        path: row.get(2)?,
        vcs_ref: row.get(3)?,
        state: from_sql_enum(row, 4)?,
        kind: from_sql_enum(row, 5)?,
    })
}

/// Fails with [`Error::Conflict`] when the project was removed meanwhile,
/// or already has a root workspace and this is another.
fn insert_workspace(conn: &Connection, workspace: &Workspace, created_at: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO workspaces (id, project_id, kind, path, vcs_ref, state, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            workspace.id,
            workspace.project_id,
            to_sql_enum(&workspace.kind),
            workspace.path,
            workspace.vcs_ref,
            to_sql_enum(&workspace.state),
            created_at
        ],
    )
    .map_err(|e| conflict(e, "the project changed meanwhile"))?;
    Ok(())
}

/// Turns a constraint violation into [`Error::Conflict`] with `message`;
/// any other error passes through.
fn conflict(e: rusqlite::Error, message: &str) -> Error {
    match e.sqlite_error_code() {
        Some(rusqlite::ErrorCode::ConstraintViolation) => Error::Conflict(message.to_string()),
        _ => Error::Sqlite(e),
    }
}

fn collect<T>(rows: impl Iterator<Item = rusqlite::Result<T>>) -> Result<Vec<T>> {
    Ok(rows.collect::<rusqlite::Result<Vec<T>>>()?)
}

/// An enum's wire name (`"git"`, `"ready"`), which is also what the database
/// stores, so there is one spelling of each value.
fn to_sql_enum<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => s,
        other => panic!("expected an enum that serializes as a string, got {other:?}"),
    }
}

/// Reads column `index` as an enum by its wire name.
fn from_sql_enum<T: DeserializeOwned>(row: &Row, index: usize) -> rusqlite::Result<T> {
    let text: String = row.get(index)?;
    serde_json::from_value(Value::String(text)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// The current time as RFC 3339 UTC with millisecond precision, the format of
/// every timestamp Core stores or sends.
pub fn now_rfc3339() -> String {
    let now = time::OffsetDateTime::now_utc();
    // Written out field by field: `time`'s RFC 3339 formatter drops trailing
    // zeros from the fraction (`.240` becomes `.24`), and a fixed width keeps
    // timestamps sortable as text. Milliseconds are enough to order lines.
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.millisecond()
    )
}

/// A new identifier: `prefix` and eight random hex digits, such as `w-1a2b3c4d`.
///
/// Short because workspace ids end up in paths and branch names. 32 random bits
/// make a collision between one user's objects very unlikely, and the primary
/// key turns one into an error rather than silent corruption.
pub fn new_id(prefix: &str) -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    format!("{prefix}-{}", &hex[..8])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(&dir.path().join("ersa.db")).expect("open");
        (dir, store)
    }

    #[test]
    fn migrations_apply_once_and_record_the_version() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("ersa.db");
        drop(Store::open(&path).expect("first open migrates"));
        // A second open finds everything applied and does nothing.
        drop(Store::open(&path).expect("second open is a no-op"));

        let conn = Connection::open(&path).expect("open");
        let version = user_version(&conn).expect("user_version");
        assert_eq!(version, MIGRATIONS.len());
        for table in ["hosts", "projects", "workspaces", "sessions", "settings"] {
            let found: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |r| r.get(0),
                )
                .expect("query");
            assert_eq!(found, 1, "table {table} exists");
        }
    }

    #[test]
    fn a_database_from_a_newer_build_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("ersa.db");
        let conn = Connection::open(&path).expect("open");
        conn.pragma_update(None, "user_version", MIGRATIONS.len() as i64 + 1)
            .expect("set version");
        drop(conn);
        assert!(matches!(Store::open(&path), Err(Error::TooNew { .. })));
    }

    /// A database as it stood after migration `version`.
    fn database_at(path: &Path, version: usize) -> Connection {
        let conn = Connection::open(path).expect("open");
        for sql in &MIGRATIONS[..version] {
            conn.execute_batch(sql).expect("old migration");
        }
        conn.pragma_update(None, "user_version", version as i64)
            .expect("set version");
        conn
    }

    #[test]
    fn migration_3_keeps_each_sessions_backend_id_and_drops_its_history() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("ersa.db");
        let conn = database_at(&path, 2);
        let init = r#"{"type":"system","subtype":"init","session_id":"from-the-log"}"#;
        let other = r#"{"type":"system","subtype":"init","session_id":"other"}"#;
        conn.execute_batch(
            "INSERT INTO hosts VALUES ('local', 'pc', 'local', '{}', 't');
             INSERT INTO projects VALUES ('p', 'local', '/r', '/r', 'r', 'git', 't');
             INSERT INTO workspaces VALUES ('w', 'p', '/w', 'b', 'ready', 't');
             INSERT INTO sessions (id, workspace_id, host_id, backend, mode, title, state,
                                   backend_session_id, created_at)
                 VALUES ('s-row', 'w', 'local', 'claude-code', 'm', 'Kept', 'ended', 'on-the-row', 't1'),
                        ('s-log', 'w', 'local', 'claude-code', NULL, NULL, 'ended', NULL, 't2'),
                        ('s-none', 'w', 'local', 'claude-code', NULL, NULL, 'error', NULL, 't3');",
        )
        .expect("old rows");
        let log = [
            ("s-row", 0, "backend", other),
            ("s-log", 0, "core", r#"{"type":"process_started"}"#),
            ("s-log", 1, "backend", "not json"),
            ("s-log", 2, "backend", init),
        ];
        for (session, idx, dir, line) in log {
            conn.execute(
                "INSERT INTO raw_log VALUES (?1, ?2, 't', ?3, ?4)",
                params![session, idx, dir, line],
            )
            .expect("old line");
        }
        conn.execute(
            "INSERT INTO projection_cache VALUES ('s-log', 1, 3, 0, '{}')",
            [],
        )
        .expect("old cache");
        drop(conn);

        drop(Store::open(&path).expect("migrates"));
        // The copy kept before it still has everything migration 3 drops.
        let backup = Connection::open(backup_path(&path, 2)).expect("the copy opens");
        let kept: i64 = backup
            .query_row("SELECT COUNT(*) FROM raw_log", [], |r| r.get(0))
            .expect("the copy has the raw log");
        assert_eq!(kept, 4);
        drop(backup);
        let conn = Connection::open(&path).expect("open");
        assert_eq!(user_version(&conn).unwrap(), MIGRATIONS.len());
        type Kept = (String, Option<String>, Option<String>);
        let rows: Vec<Kept> = conn
            .prepare("SELECT id, backend_session_id, mode FROM sessions ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let kept = |id: &str, backend_id: Option<&str>, mode: Option<&str>| {
            (
                id.to_string(),
                backend_id.map(String::from),
                mode.map(String::from),
            )
        };
        assert_eq!(
            rows,
            [
                kept("s-log", Some("from-the-log"), None),
                kept("s-none", None, None),
                kept("s-row", Some("on-the-row"), Some("m")),
            ]
        );
        for gone in ["raw_log", "projection_cache"] {
            let found: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name = ?1",
                    [gone],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(found, 0, "{gone} is gone");
        }
        let columns: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('sessions')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            columns,
            [
                "id",
                "workspace_id",
                "host_id",
                "backend",
                "backend_session_id",
                "mode",
                // Migration 5's.
                "model",
                "effort",
                "permission_mode",
                // Migration 6's.
                "archived",
                // Migration 7's.
                "deleting",
                "delete_files",
            ]
        );
    }

    #[tokio::test]
    async fn settings_patches_merge_and_persist() {
        let (_dir, store) = temp_store();
        let patch = |v: Value| v.as_object().cloned().expect("object");
        // Absolute on whatever platform runs the test.
        let root = std::env::temp_dir().join("wt").display().to_string();
        store
            .patch_settings(patch(
                json!({ "workspaces": { "root": root }, "theme": "dark" }),
            ))
            .await
            .expect("patch");
        let after = store
            .patch_settings(patch(
                json!({ "theme": null, "workspaces": { "hook": "x" } }),
            ))
            .await
            .expect("patch");
        let expected = json!({ "workspaces": { "root": root, "hook": "x" } });
        assert_eq!(Value::Object(after), expected);
        assert_eq!(
            Value::Object(store.settings().await.expect("get")),
            expected
        );
    }

    #[tokio::test]
    async fn workspace_transitions_only_from_the_given_states() {
        let (_dir, store) = temp_store();
        store.ensure_local_host("pc".into()).await.expect("host");
        let project = Project {
            id: "p".into(),
            host_id: LOCAL_HOST_ID.into(),
            path: "/r".into(),
            name: "r".into(),
            vcs: Vcs::Git,
            trusted: false,
        };
        store
            .insert_project(project.clone(), "/r".into())
            .await
            .expect("project");
        // The same key again is a conflict, not an internal error.
        let again = Project {
            id: "p2".into(),
            ..project
        };
        assert!(matches!(
            store.insert_project(again, "/r".into()).await,
            Err(Error::Conflict(_))
        ));
        store
            .insert_workspace(Workspace {
                id: "w".into(),
                project_id: "p".into(),
                kind: WorkspaceKind::Isolated,
                path: "/w".into(),
                vcs_ref: "b".into(),
                state: WorkspaceState::Ready,
            })
            .await
            .expect("workspace");

        let ready: &[WorkspaceState] = &[WorkspaceState::Ready];
        assert!(
            store
                .transition_workspace("w".into(), ready, WorkspaceState::Removing)
                .await
                .expect("first")
        );
        assert!(
            !store
                .transition_workspace("w".into(), ready, WorkspaceState::Removing)
                .await
                .expect("second")
        );

        // A project with a workspace cannot be deleted: a conflict.
        assert!(matches!(
            store.delete_project("p".into()).await,
            Err(Error::Conflict(_))
        ));

        // At startup, an interrupted removal becomes `error`.
        assert_eq!(store.reconcile_workspaces().await.expect("reconcile"), 1);
        let workspace = store
            .workspace("w".into())
            .await
            .expect("get")
            .expect("row");
        assert_eq!(workspace.state, WorkspaceState::Error);
    }

    // verifies: PROTO-CORE-API-R11
    #[test]
    fn migration_4_leaves_every_existing_project_untrusted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("ersa.db");
        let conn = database_at(&path, 3);
        conn.execute_batch(
            "INSERT INTO hosts VALUES ('local', 'pc', 'local', '{}', 't');
             INSERT INTO projects VALUES ('p', 'local', '/r', '/r', 'r', 'git', 't');",
        )
        .expect("old rows");
        drop(conn);

        drop(Store::open(&path).expect("migrates"));
        let conn = Connection::open(&path).expect("reopen");
        let trusted: bool = conn
            .query_row("SELECT trusted FROM projects WHERE id = 'p'", [], |row| {
                row.get(0)
            })
            .expect("row");
        assert!(!trusted);
    }

    #[tokio::test]
    async fn migration_5_gives_every_existing_project_its_root_workspace_listed_first() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("ersa.db");
        let conn = database_at(&path, 4);
        conn.execute_batch(
            "INSERT INTO hosts VALUES ('local', 'pc', 'local', '{}', 't');
             INSERT INTO projects VALUES ('p', 'local', '/r', '/r', 'r', 'git', 't0', 1);
             INSERT INTO workspaces VALUES ('w-old', 'p', '/w', 'ersa/w-old', 'ready', 't1');
             INSERT INTO sessions VALUES ('s', 'w-old', 'local', 'claude-code', NULL, NULL);",
        )
        .expect("old rows");
        drop(conn);

        let store = Store::open(&path).expect("migrates");
        let workspaces = store.workspaces("p".into()).await.expect("list");
        assert_eq!(workspaces.len(), 2);
        let root = &workspaces[0];
        assert_eq!(
            (root.kind, root.path.as_str(), root.state),
            (WorkspaceKind::Root, "/r", WorkspaceState::Ready)
        );
        assert!(
            root.id.starts_with("w-") && root.id.len() == 10,
            "{}",
            root.id
        );
        assert_eq!(workspaces[1].kind, WorkspaceKind::Isolated);
        // A session from before recorded no model, effort or permission mode.
        let session = store.session("s".into()).await.expect("get").expect("row");
        assert_eq!(
            (session.model, session.effort, session.permission_mode),
            (None, None, None)
        );
        // A project has one root workspace, never two.
        let second = Workspace {
            id: "w-2".into(),
            ..root.clone()
        };
        assert!(matches!(
            store.insert_workspace(second).await,
            Err(Error::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn a_project_comes_and_goes_with_its_root_workspace() {
        let (_dir, store) = temp_store();
        store.ensure_local_host("pc".into()).await.expect("host");
        let project = Project {
            id: "p".into(),
            host_id: LOCAL_HOST_ID.into(),
            path: "/r".into(),
            name: "r".into(),
            vcs: Vcs::None,
            trusted: false,
        };
        let root = store
            .insert_project(project, "/r".into())
            .await
            .expect("insert");
        assert_eq!(
            store.workspaces("p".into()).await.expect("list"),
            vec![root.clone()]
        );
        assert_eq!((root.kind, root.path.as_str()), (WorkspaceKind::Root, "/r"));

        store.delete_project("p".into()).await.expect("delete");
        assert_eq!(store.workspace(root.id).await.expect("get"), None);
        assert_eq!(store.project("p".into()).await.expect("get"), None);
    }

    // verifies: PROTO-CORE-API-R11
    #[tokio::test]
    async fn project_trust_is_stored_as_given_and_changed_only_when_set() {
        let (_dir, store) = temp_store();
        store.ensure_local_host("pc".into()).await.expect("host");
        let project = Project {
            id: "p".into(),
            host_id: LOCAL_HOST_ID.into(),
            path: "/r".into(),
            name: "r".into(),
            vcs: Vcs::Git,
            trusted: true,
        };
        store
            .insert_project(project.clone(), "/r".into())
            .await
            .expect("insert");
        assert_eq!(
            store.project("p".into()).await.expect("get"),
            Some(project.clone())
        );

        let changed = store
            .set_project_trust("p".into(), false)
            .await
            .expect("set");
        let untrusted = Project {
            trusted: false,
            ..project
        };
        assert_eq!(changed, Some(untrusted.clone()));
        assert_eq!(store.projects().await.expect("list"), vec![untrusted]);
        assert_eq!(
            store
                .set_project_trust("nope".into(), true)
                .await
                .expect("set"),
            None
        );
    }

    #[test]
    fn ids_are_short_and_prefixed() {
        let id = new_id("w");
        assert_eq!(id.len(), 10);
        assert!(id.starts_with("w-"));
        assert_ne!(new_id("w"), new_id("w"));
    }

    #[test]
    fn timestamps_are_rfc3339_utc_milliseconds() {
        let ts = now_rfc3339();
        // `2026-10-31T12:00:00.123Z`: UTC, milliseconds, nothing else.
        assert_eq!(ts.len(), 24, "{ts}");
        assert!(ts.ends_with('Z') && ts.as_bytes()[19] == b'.', "{ts}");
    }
}
