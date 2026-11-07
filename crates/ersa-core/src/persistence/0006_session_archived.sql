-- Migration 6: the user's arrangement of a session starts with whether it
-- is archived (ADR 0026). Core stores it because neither backend's record
-- has a place for it; the backend's own tools still list an archived
-- session. Every session so far is not archived.
ALTER TABLE sessions ADD COLUMN archived INTEGER NOT NULL DEFAULT 0
    CHECK (archived IN (0, 1));
