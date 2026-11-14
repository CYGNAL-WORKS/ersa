-- When Core last gave a session's backend its model, effort and permission
-- mode, on the host's clock (PROTO-CORE-API-R71): RFC 3339, NULL for a value
-- Core never gave, and for every session from before Core kept the time.
ALTER TABLE sessions ADD COLUMN model_at TEXT;
ALTER TABLE sessions ADD COLUMN effort_at TEXT;
ALTER TABLE sessions ADD COLUMN permission_mode_at TEXT;
