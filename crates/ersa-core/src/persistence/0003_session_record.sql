-- Migration 3: a session's history is the backend's own session record
-- (ADR 0017), so Ersa no longer keeps a copy of it. The raw log and the
-- projection cache go, and so does what the sessions' rows summarised of
-- them (state, tile, last event), and its title and creation time, which
-- the backend's record gives. A session keeps what finds it again (host,
-- workspace and so project, backend, and the backend's own session id) and
-- the one thing about it that is Ersa's own: its mode.

-- The backend's id was copied to the row as the projection saw it. Should a
-- row have missed it, it is still in the session's first `system/init` line,
-- which is how Claude Code announces it; read it from there before the raw
-- log goes.
UPDATE sessions
SET backend_session_id = (
    SELECT json_extract(line, '$.session_id')
    FROM raw_log
    WHERE raw_log.session_id = sessions.id
      AND dir = 'backend'
      AND json_valid(line)
      AND json_extract(line, '$.type') = 'system'
      AND json_extract(line, '$.subtype') = 'init'
    ORDER BY idx
    LIMIT 1
)
WHERE backend_session_id IS NULL AND backend = 'claude-code';

DROP TRIGGER raw_log_is_append_only;
DROP TRIGGER raw_log_is_never_rewritten;
DROP TABLE projection_cache;
DROP TABLE raw_log;

-- SQLite drops columns one at a time and not at all from some tables, so
-- the table is rebuilt with the columns it keeps.
CREATE TABLE sessions_kept (
    id                  TEXT PRIMARY KEY,
    workspace_id        TEXT NOT NULL REFERENCES workspaces (id),
    host_id             TEXT NOT NULL REFERENCES hosts (id),
    backend             TEXT NOT NULL,
    -- The backend's own session id, which finds its session record; unknown
    -- until the backend announces it.
    backend_session_id  TEXT,
    -- Ersa's mode for the session (milestone M3).
    mode                TEXT
);

INSERT INTO sessions_kept (id, workspace_id, host_id, backend, backend_session_id, mode)
SELECT id, workspace_id, host_id, backend, backend_session_id, mode
FROM sessions;

DROP TABLE sessions;
ALTER TABLE sessions_kept RENAME TO sessions;
