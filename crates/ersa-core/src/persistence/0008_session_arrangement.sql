-- Migration 8: the rest of the user's arrangement of sessions (ADR 0026):
-- the sections the user makes, and per session whether it is pinned, its
-- section, and when its latest turn ended. Every session so far is
-- unpinned, in no section, with no turn ended that Core recorded.

-- `name_key` is the name as sections are compared, case folded by Core, so
-- two sections never share a name ignoring case, even when two requests race.
CREATE TABLE sections (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    name_key    TEXT NOT NULL UNIQUE,
    created_at  TEXT NOT NULL
);

ALTER TABLE sessions ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0
    CHECK (pinned IN (0, 1));
-- NULL for no section. `section.remove` clears it in the same transaction
-- that removes the section, so the reference never dangles.
ALTER TABLE sessions ADD COLUMN section_id TEXT REFERENCES sections (id);
-- RFC 3339; NULL until one of the session's turns has ended.
ALTER TABLE sessions ADD COLUMN last_activity_at TEXT;
