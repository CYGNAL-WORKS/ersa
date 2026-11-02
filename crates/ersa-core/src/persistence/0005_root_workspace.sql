-- Migration 5: every project has a root workspace, its own folder, where
-- sessions can run without an isolated checkout; and a session keeps the
-- model, effort and permission mode it started with (PROTO-CORE-API).

-- Every workspace so far was a checkout Core made for sessions.
ALTER TABLE workspaces ADD COLUMN kind TEXT NOT NULL DEFAULT 'isolated'
    CHECK (kind IN ('root', 'isolated'));

-- One root workspace per project, never two.
CREATE UNIQUE INDEX workspaces_one_root ON workspaces (project_id) WHERE kind = 'root';

-- Projects added before this migration get theirs now: the project's own
-- path, always ready. Its id is made like Core's (`w-` and eight random hex
-- digits). Its `vcs_ref` is empty: the root's checkout is the user's.
INSERT INTO workspaces (id, project_id, path, vcs_ref, state, created_at, kind)
SELECT 'w-' || lower(hex(randomblob(4))), id, path, '', 'ready', created_at, 'root'
FROM projects;

-- The ids from `backend.list` the session started with. NULL for sessions
-- created before they were recorded, which started with the backend's
-- defaults; Core reports those defaults for them.
ALTER TABLE sessions ADD COLUMN model TEXT;
ALTER TABLE sessions ADD COLUMN effort TEXT;
ALTER TABLE sessions ADD COLUMN permission_mode TEXT;
