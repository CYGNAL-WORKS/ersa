-- A fork's conversation is in its original's record until its own first
-- message makes the backend write one (FLOW-FORK-SESSION-R01).
ALTER TABLE sessions ADD COLUMN forked_from TEXT;
ALTER TABLE sessions ADD COLUMN forked_through TEXT;
-- A fork's copied entries keep the original's times, so its record can't say
-- when the fork began.
ALTER TABLE sessions ADD COLUMN forked_at TEXT;
-- A fork's title until its own record gives one: nothing else can say it once
-- Core restarts, since the fork has no record before its first message.
ALTER TABLE sessions ADD COLUMN forked_title TEXT;
