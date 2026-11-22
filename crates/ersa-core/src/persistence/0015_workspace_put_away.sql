-- Migration 15: an isolated workspace can be put away (FLOW-CLEAN-UP-R02):
-- its folder removed, the workspace and its branch kept, with what the
-- folder had checked out stored so it comes back as it was. Exactly one of
-- `put_away_branch` (a branch's name) and `put_away_commit` (a commit id,
-- for a folder on no branch) is set while the workspace is put away.

-- SQLite cannot change a CHECK constraint in place, so the table is
-- rebuilt. Sessions refer to it, so the foreign keys are checked at the
-- end of this transaction rather than at each statement: dropping the old
-- table counts each session row as dangling, and copying the rows back
-- into the new one counts them found again.
PRAGMA defer_foreign_keys = ON;

CREATE TABLE workspaces_before AS SELECT rowid AS old_rowid, * FROM workspaces;
DROP INDEX workspaces_one_root;
DROP TABLE workspaces;

CREATE TABLE workspaces (
    id               TEXT PRIMARY KEY,
    project_id       TEXT NOT NULL REFERENCES projects (id),
    path             TEXT NOT NULL,
    vcs_ref          TEXT NOT NULL,
    state            TEXT NOT NULL
                     CHECK (state IN ('ready', 'creating', 'removing', 'error', 'put_away')),
    created_at       TEXT NOT NULL,
    kind             TEXT NOT NULL DEFAULT 'isolated' CHECK (kind IN ('root', 'isolated')),
    put_away_branch  TEXT,
    put_away_commit  TEXT,
    CHECK (put_away_branch IS NULL OR put_away_commit IS NULL)
);

-- In the order they were stored, so nothing that breaks a tie by row
-- changes.
INSERT INTO workspaces (rowid, id, project_id, path, vcs_ref, state, created_at, kind)
SELECT old_rowid, id, project_id, path, vcs_ref, state, created_at, kind
FROM workspaces_before
ORDER BY old_rowid;

DROP TABLE workspaces_before;
CREATE UNIQUE INDEX workspaces_one_root ON workspaces (project_id) WHERE kind = 'root';
