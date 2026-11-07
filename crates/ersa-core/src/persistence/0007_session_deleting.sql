-- Migration 7: a session the user is deleting (FLOW-ARCHIVE-DELETE-SESSION).
-- `deleting` is `running` while Core works on the delete and `failed` once a
-- step failed; NULL for every other session. `delete_files` is whether the
-- delete takes the workspace's folder and branch, decided when the session
-- is first marked and only ever narrowed after; NULL until then.
ALTER TABLE sessions ADD COLUMN deleting TEXT
    CHECK (deleting IN ('running', 'failed'));
ALTER TABLE sessions ADD COLUMN delete_files INTEGER
    CHECK (delete_files IN (0, 1));
