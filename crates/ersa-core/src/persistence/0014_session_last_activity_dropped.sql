-- Migration 14: Core stores no last activity (PROTO-CORE-API-R75). A
-- session's last activity comes from its backend's record, and while it is
-- live from its turns' ends, held in memory only (ADR 0033). The column has
-- no index or constraint, so SQLite drops it in place.
ALTER TABLE sessions DROP COLUMN last_activity_at;
