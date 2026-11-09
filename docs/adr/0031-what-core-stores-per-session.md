# 0031: What Core stores per session, and what it asks the backend

- Status: accepted
- Date: 2026-11-09
- Supersedes: 0026
- Related briefs: PROTO-CORE-API, PROTO-CLAUDE-CODE-STREAM, FLOW-ORGANIZE-SESSIONS, FLOW-ARCHIVE-DELETE-SESSION, FLOW-FORK-SESSION, FLOW-NEW-SESSION, ARCH-CORE, LAY-SHELL

## Context

The harness rule says Ersa never adds to or duplicates what a backend CLI owns. An audit on 2026-11-09 found Core's store had grown past ADR 0026's list: the model, effort and permission mode a session started with, shown for good even when the backend's values had changed, and the time a session's latest turn ended, which misses turns taken outside Ersa. The CLI reports its models before the first message, reports the model and permission mode a session really runs with, takes changes to both between turns, and its record dates every message.

## Options considered

1. **Keep ADR 0026's list and add the columns to it**: no work. Ersa keeps showing values the backend may contradict.
2. **Store nothing the backend can answer**: one truth. The record holds a session's model and permission mode from its first message on, but not its effort, and nothing exists to read before that message.
3. **Ask the backend for what it reports, and store only what relaunches a session**: the shown values are the backend's while it can answer, and the store holds no second truth for them.

## Decision

Option 3, and ADR 0026's decision otherwise stands, restated here so this record is whole: a session's history is the backend's own session record, read through the adapter, and details only the live stream carries are shown and not stored; Core's operational records go to an app log; Ersa sessions are ordinary backend sessions, launched the way the vendor's own IDE integration launches them (for Claude Code, the user's settings load, `CLAUDE_CODE_ENTRYPOINT` is `ersa`, and inherited Claude variables are cleared); Ersa lists and resumes any session in the backend's record for a project, and its own sessions can be resumed elsewhere by id; retention is the backend's, except that the user's Delete deletes the record. The title and the history stay the backend's.

Per session, Core stores what finds it again; the user's arrangement of it (whether it is pinned, its section, whether it is archived, whether it is unseen); and the values to relaunch it with (model, effort, permission mode). Core stores the user's sections. The rail's grouping, sort, filter and collapsed groups are user settings in Core.

Core reports the backend's own values once the backend has given them: the model and permission mode of a live session, its list of models, and a session's last activity, which comes from its record. Relaunch values change when the user changes them, and are shown only when the backend has reported nothing.

A workspace stays Ersa's, outside the harness rule: Ersa makes one for every backend it drives, including a backend with no worktree of its own, though a CLI may offer one.

## Consequences

- ADR 0026's context and consequences still hold, among them why a session started elsewhere starts with no pin, no section and not archived, and that Delete is the one exception to retention being the backend's.
- ADR 0017's rule becomes: per session, Ersa stores what finds it again, the user's arrangement of it and the values to relaunch it with, never its history, its title or its last activity.
- Sorting by last activity counts work done outside Ersa, read when the rail next lists sessions.
- A session's effort stays what the user asked for, since the CLI reports none and no way to change it was recorded (2.1.316).
- Revisit if a backend's record gains supported fields for the arrangement, or a CLI reports a session's effort.
