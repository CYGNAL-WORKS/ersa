# 0031: What Core stores per session, and what it asks the backend

- Status: accepted
- Date: 2026-11-09
- Supersedes: 0026
- Related briefs: PROTO-CORE-API, PROTO-CLAUDE-CODE-STREAM, FLOW-ORGANIZE-SESSIONS, FLOW-ARCHIVE-DELETE-SESSION, FLOW-FORK-SESSION, FLOW-NEW-SESSION, ARCH-CORE, LAY-SHELL

## Context

The harness rule says Ersa never adds to or duplicates what a backend CLI owns. An audit on 2026-11-09 found Core's store had grown past ADR 0026's list: the model, effort and permission mode a session started with, shown for good even when the backend's values had changed, and the time a session's latest turn ended, which misses turns taken outside Ersa. The CLI reports its models before the first message and reports the model and permission mode a session really runs with. It takes changes to model, effort and permission mode between turns. From a session's first message on, its record holds all three and dates every message; before that message there is no record to read.

## Options considered

1. **Keep ADR 0026's list and add the columns to it**: no work. Ersa keeps showing values the backend may contradict.
2. **Keep a copy of each session's values to relaunch it with**: a stopped session relaunches as Ersa last saw it. A change made in the terminal is shown wrongly and overridden at the next launch.
3. **Ask the backend, and store a session's values only until it has a record**: the shown and relaunched values are the backend's, and Core's copy lasts only while nothing else holds them.

## Decision

Option 3 (owner's decision, 2026-11-09), and ADR 0026's decision otherwise stands, restated here so this record is whole: a session's history is the backend's own session record, read through the adapter, and details only the live stream carries are shown and not stored; Core's operational records go to an app log; Ersa sessions are ordinary backend sessions, launched the way the vendor's own IDE integration launches them (for Claude Code, the user's settings load, `CLAUDE_CODE_ENTRYPOINT` is `ersa`, and inherited Claude variables are cleared); Ersa lists and resumes any session in the backend's record for a project, and its own sessions can be resumed elsewhere by id; retention is the backend's, except that the user's Delete deletes the record. The title and the history stay the backend's.

Per session, Core stores what finds it again; the user's arrangement of it (whether it is pinned, its section, whether it is archived, whether it is unseen); and, only until the session has a record, the model, effort and permission mode it was asked to start with. Core stores the user's sections. The rail's grouping, sort, filter and collapsed groups are user settings in Core.

Core reports the backend's own values: its list of models, a live session's model and permission mode from the stream, a session's last activity from its record, and, once a session has a record, the model, effort and permission mode it holds, which are also what the session is relaunched with.

A workspace stays Ersa's, outside the harness rule: Ersa makes one for every backend it drives, including a backend with no worktree of its own, though a CLI may offer one.

## Consequences

- ADR 0026's context and consequences still hold, among them why a session started elsewhere starts with no pin, no section and not archived, and that Delete is the one exception to retention being the backend's.
- ADR 0017's rule becomes: per session, Ersa stores what finds it again, the user's arrangement of it and, until it has a record, the values it was asked to start with, never its history, its title, its last activity or, once recorded, its model, effort or permission mode.
- Sorting by last activity counts work done outside Ersa, read when the rail next lists sessions.
- A change made in the terminal shows in Ersa and carries into its next launch.
- These values rest on fields inside the CLI's record, which is internal to it and pinned by fixtures per version (ADR 0017).
- Revisit if a backend's record gains supported fields for the arrangement, or stops carrying these values.
