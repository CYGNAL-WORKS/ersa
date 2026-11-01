-- Migration 1: the initial tables.
--
-- Identifiers are the strings Core issues on the wire. Times are RFC 3339 UTC
-- text. JSON columns hold serde_json output. CHECK constraints mirror the
-- enums in core-api.schema.json so a bad value fails at the database, not later.

-- Machines sessions can run on. The local host always exists, with id 'local'.
CREATE TABLE hosts (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('local', 'ssh')),
    -- Connection details for remote hosts (milestone M4); '{}' for the local one.
    config      TEXT NOT NULL DEFAULT '{}',
    created_at  TEXT NOT NULL
);

CREATE TABLE projects (
    id          TEXT PRIMARY KEY,
    host_id     TEXT NOT NULL REFERENCES hosts (id),
    path        TEXT NOT NULL,
    name        TEXT NOT NULL,
    vcs         TEXT NOT NULL CHECK (vcs IN ('git', 'perforce', 'none')),
    created_at  TEXT NOT NULL,
    UNIQUE (host_id, path)
);

CREATE TABLE workspaces (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects (id),
    path        TEXT NOT NULL,
    vcs_ref     TEXT NOT NULL,
    state       TEXT NOT NULL CHECK (state IN ('ready', 'creating', 'removing', 'error')),
    created_at  TEXT NOT NULL
);

CREATE TABLE sessions (
    id                  TEXT PRIMARY KEY,
    workspace_id        TEXT NOT NULL REFERENCES workspaces (id),
    host_id             TEXT NOT NULL REFERENCES hosts (id),
    backend             TEXT NOT NULL,
    mode                TEXT,
    title               TEXT,
    state               TEXT NOT NULL CHECK (state IN (
                            'starting', 'running', 'waiting_user', 'idle',
                            'disconnected', 'ended', 'error')),
    -- The backend's own session id, for resume; unknown until it announces it.
    backend_session_id  TEXT,
    created_at          TEXT NOT NULL
);

-- Every line a session's backend emitted, Ersa wrote to it, or Core recorded
-- about its lifecycle, verbatim and in order (ADR 0010). The persisted truth
-- of a session: everything else about it is projected from here.
CREATE TABLE raw_log (
    session_id  TEXT NOT NULL REFERENCES sessions (id),
    -- 0, 1, 2, ... with no gaps within a session, across all directions.
    idx         INTEGER NOT NULL CHECK (idx >= 0),
    ts          TEXT NOT NULL,
    dir         TEXT NOT NULL CHECK (dir IN ('backend', 'host', 'core')),
    line        TEXT NOT NULL,
    PRIMARY KEY (session_id, idx)
) WITHOUT ROWID;

-- Append-only: a line, once written, never changes. Deleting is allowed, so
-- removing a session can remove its log.
CREATE TRIGGER raw_log_is_append_only
BEFORE UPDATE ON raw_log
BEGIN
    SELECT RAISE(ABORT, 'raw_log is append-only');
END;

-- Projected events, cached. Keyed by the agent event schema version (v) and
-- the adapter's projection version as well as the session, so a change to
-- either leaves stale rows unused until they are re-derived (ADR 0010).
CREATE TABLE projection_cache (
    session_id          TEXT NOT NULL REFERENCES sessions (id),
    v                   INTEGER NOT NULL,
    projection_version  INTEGER NOT NULL,
    seq                 INTEGER NOT NULL CHECK (seq >= 0),
    -- The whole PROTO-AGENT-EVENTS envelope as JSON.
    envelope            TEXT NOT NULL,
    PRIMARY KEY (session_id, v, projection_version, seq)
) WITHOUT ROWID;

-- User settings: one row per top-level key of the settings object, with the
-- value as JSON.
CREATE TABLE settings (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);
