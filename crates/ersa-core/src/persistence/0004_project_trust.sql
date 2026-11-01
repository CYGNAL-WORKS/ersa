-- Migration 4: whether the user trusts a project, so that its own
-- configuration loads in its sessions (ADR 0018). Projects added before Ersa
-- asked were never answered for, so they start untrusted, like every project
-- the user has not said they trust (PROTO-CORE-API-R11).
ALTER TABLE projects ADD COLUMN trusted INTEGER NOT NULL DEFAULT 0 CHECK (trusted IN (0, 1));
