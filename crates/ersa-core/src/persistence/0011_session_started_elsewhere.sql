-- Migration 11: sessions started in the backend's own tools, which Core lists
-- from the backend's records for a project's folders (FLOW-RESUME-SESSION-R12).
-- Core stores one only to keep its id and the user's arrangement of it (ADR
-- 0033), and tells it from a session Ersa started: such a session leaves the
-- list when its record is gone (R06) and never holds back removing its
-- workspace (R09). Every session so far was started in Ersa.
ALTER TABLE sessions ADD COLUMN started_elsewhere INTEGER NOT NULL DEFAULT 0
    CHECK (started_elsewhere IN (0, 1));
