# 0010: The raw vendor stream is the persisted truth; Ersa events are a versioned projection

- Status: superseded by 0017
- Date: 2026-10-31
- Related briefs: ARCH-CORE, PROTO-AGENT-EVENTS

## Context

The first architecture draft persisted translated Ersa events as the session's event log and reconstructed transcripts from them. Vendor protocols change shape between CLI releases; any transcript translated before an adapter fix would be permanently wrong, and the "native passthrough" event would become a dumping ground. Each backend also keeps its own session state (Claude Code writes per-session files it resumes from), so Ersa's translated log and the backend's own record could disagree, with the backend winning.

## Options considered

1. **Persist translated events only**: compact and uniform. Loses information on every translation bug and cannot be re-derived.
2. **Persist the raw vendor stream, derive Ersa events on read**: lossless; a translation fix is a re-projection, not a migration. Costs a projection step on load and more storage.
3. **Persist both, raw as truth and projection as a cache tagged with its projection version**: option 2 with a materialised cache.

## Decision

Option 3. Every line the backend emits is stored verbatim, in order, per session, tagged with backend and version. The Ersa event stream is a projection with a version number; the projection is cached and invalidated when its version changes. The transcript, the rail state, and resume are all derived from the projection, and the projection is derivable from the raw log alone.

## Consequences

A vendor protocol change costs an adapter fix and a re-projection, never a data migration. The developer pane shows the raw log for free. Remote backfill after a disconnect is a file copy of the backend's own session record. Storage grows with raw output, which is acceptable for text. ARCH-CORE's requirement that a transcript be reconstructible from the event log is restated against the raw log plus a projection version.
