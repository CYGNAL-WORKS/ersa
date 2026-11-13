# 0033: What Core stores per session, and what it asks the backend

- Status: accepted
- Date: 2026-11-09
- Supersedes: 0026
- Related briefs: PROTO-CORE-API, PROTO-CLAUDE-CODE-STREAM, FLOW-ORGANIZE-SESSIONS, FLOW-ARCHIVE-DELETE-SESSION, FLOW-FORK-SESSION, FLOW-NEW-SESSION, ARCH-CORE, LAY-SHELL

## Context

The harness rule says Ersa never adds to or duplicates what a backend CLI owns. An audit on 2026-11-09 found Core's store had grown past ADR 0026's list: the model, effort and permission mode a session started with, shown for good even when the backend's values had changed, and the time a session's latest turn ended, which misses turns taken outside Ersa. The CLI reports its models before the first message and reports the model and permission mode a session really runs with. It takes changes to model, effort and permission mode between turns. Its record dates every message and holds the permission mode on each prompt and the model and effort on each reply, so a value Ersa asks for shows there only once a turn has run with it; until the first reply the record holds no model or effort at all.

## Options considered

1. **Keep ADR 0026's list and add the columns to it**: no work. Ersa keeps showing values the backend may contradict.
2. **Keep a copy of each session's values to relaunch it with**: a stopped session relaunches as Ersa last saw it. A change made in the terminal is shown wrongly and overridden at the next launch.
3. **Read only the record**: no copy at all. A value chosen in Ersa is lost when the session stops before a turn runs with it, and a record whose latest prompts are far back leaves no permission mode to restart with.
4. **Ask the backend, and keep what Ersa asked for only until the record shows a newer turn**: the shown and relaunched values are the backend's once a turn has run with them, and Ersa's note covers only what the backend has not recorded yet.

## Decision

Option 4 (owner's decisions, 2026-11-09 and 2026-11-12), and ADR 0026's decision otherwise stands, restated here so this record is whole: a session's history is the backend's own session record, read through the adapter, and details only the live stream carries are shown and not stored; Core's operational records go to an app log; Ersa sessions are ordinary backend sessions, launched the way the vendor's own IDE integration launches them (for Claude Code, the user's settings load, `CLAUDE_CODE_ENTRYPOINT` is `ersa`, and inherited Claude variables are cleared); Ersa lists and resumes any session in the backend's record for a project, and its own sessions can be resumed elsewhere by id; retention is the backend's, except that the user's Delete deletes the record. The title and the history stay the backend's.

Per session, Core stores what finds it again; the user's arrangement of it (whether it is pinned, its section, whether it is archived, whether it is unseen); and the model, effort and permission mode Core last asked the backend for, each with the time it asked. Core stores the user's sections. The rail's grouping, sort, filter and collapsed groups are user settings in Core.

Core reports the backend's own values: its list of models, a live session's model and permission mode from the stream, and a session's last activity from its record. For a session's model, effort and permission mode, the record's value wins once the record shows a turn that began after Core's request; until then the request stands. What Core reports is also what a relaunch passes.

A workspace stays Ersa's, outside the harness rule: Ersa makes one for every backend it drives, including a backend with no worktree of its own, though a CLI may offer one.

## Consequences

- ADR 0026's context and consequences still hold, among them why a session started elsewhere starts with no pin, no section and not archived, and that Delete is the one exception to retention being the backend's.
- ADR 0017's rule becomes: per session, Ersa stores what finds it again, the user's arrangement of it and what it last asked the backend for, which gives way to any newer turn in the record; never its history, its title or its last activity.
- Sorting by last activity counts work done outside Ersa, read when the rail next lists sessions.
- A change made in the terminal shows in Ersa and carries into its next launch, and a value chosen in Ersa survives a stop before any turn has run with it.
- These values rest on fields inside the CLI's record, which is internal to it and pinned by fixtures per version (ADR 0017).
- Revisit if a backend's record gains supported fields for the arrangement, or stops carrying these values.
