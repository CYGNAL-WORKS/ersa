# 0026: Ersa keeps the user's arrangement of its sessions

- Status: superseded by 0031
- Date: 2026-11-05
- Supersedes: 0017
- Related briefs: FLOW-ORGANIZE-SESSIONS, FLOW-ARCHIVE-DELETE-SESSION, FLOW-FORK-SESSION, PROTO-CORE-API, ARCH-CORE, LAY-SHELL

## Context

ADR 0017 limits what Ersa stores per session to what finds it again: host, project, workspace, backend, the backend's session id, and its mode. The owner wants the rail organized: pinned sessions, user-made sections, archived sessions, deleted sessions that stay gone, and a chosen grouping, sort, filter and set of collapsed groups, all kept across restarts. None of this is in a backend's session record, and Claude Code's record has no field for any of it. A session's title stays the backend's.

## Options considered

1. **Keep it in the Interface** (browser storage in the webview): no Core change. Lost when the Shell's data is cleared, invisible to a second window, and a second store beside Core's.
2. **Write it into the backend's record**: one truth with the backend's own tools. The record's format is internal for Claude Code (ADR 0017), Copilot's has no place for it, and Ersa would be writing another product's files.
3. **Keep it in Core's store, as the user's arrangement of sessions**: one store, survives restarts, any Interface sees it. Extends ADR 0017's per-session list.

## Decision

Option 3, and ADR 0017's decision otherwise stands, restated here so this record is whole: a session's history is the backend's own session record, read through the adapter, and details only the live stream carries are shown and not stored; Core's operational records go to an app log; Ersa sessions are ordinary backend sessions, launched the way the vendor's own IDE integration launches them (for Claude Code, the user's settings load, `CLAUDE_CODE_ENTRYPOINT` is `ersa`, and inherited Claude variables are cleared); Ersa lists and resumes any session in the backend's record for a project, and its own sessions can be resumed elsewhere by id; retention is the backend's, except that the user's Delete deletes the record.

For the arrangement: per session, Core also stores whether it is pinned, its section and whether it is archived, and, beside the arrangement, the time its latest turn ended, which sorting reads. Core stores the user's sections. The rail's grouping, sort, filter and collapsed groups are user settings in Core. The title and the history stay the backend's.

## Consequences

- ADR 0017's context and consequences still hold, among them that a session's view is the record plus the live stream merged on the backend's message ids, that the record's wrapper is pinned by fixtures per CLI version, and why the entrypoint is `ersa`.
- ADR 0017's rule becomes: per session, Ersa stores what finds it again and the user's arrangement of it, never its history or its title.
- A session started elsewhere and first listed by Ersa (FLOW-RESUME-SESSION) starts with no pin, no section and not archived.
- Deleting a session in Ersa deletes it: its record goes from the backend through the adapter, so neither Ersa nor the backend's own tools list it again. This is the one exception to retention being the backend's (ADR 0017), and it is the user's explicit act, after a question.
- Archive is Ersa's: the backend's own tools still list and resume an archived session.
- Revisit if a backend's record gains supported fields for pins, folders or archiving, when sharing the arrangement with the backend's own tools would be worth the dependency.
