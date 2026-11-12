-- A fork's conversation is in its original's record until its own first
-- message makes the backend write one (FLOW-FORK-SESSION-R01).
ALTER TABLE sessions ADD COLUMN forked_from TEXT;
ALTER TABLE sessions ADD COLUMN forked_through TEXT;
