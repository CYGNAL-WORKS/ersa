-- Migration 12: whether the session is unseen, the rest of the user's
-- arrangement Core stores (ADR 0033, PROTO-CORE-API-R78). Every session so
-- far is not unseen, as a session Core lists for the first time is not.
ALTER TABLE sessions ADD COLUMN unseen INTEGER NOT NULL DEFAULT 0
    CHECK (unseen IN (0, 1));
