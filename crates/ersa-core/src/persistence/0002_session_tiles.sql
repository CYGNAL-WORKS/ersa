-- Migration 2: what session.list reports about a session's events, kept on
-- its row so listing sessions never has to read their projections.
--
-- Both are derived from the projection and written in the same transaction
-- as the events they summarise, so they never disagree with the cache.

-- The tile object (PROTO-CORE-API, Sessions) as JSON.
ALTER TABLE sessions ADD COLUMN tile TEXT NOT NULL DEFAULT '{"pending":[]}';

-- The seq of the session's last event; NULL before the first.
ALTER TABLE sessions ADD COLUMN last_event_seq INTEGER CHECK (last_event_seq >= 0);
