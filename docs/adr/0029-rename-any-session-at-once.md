# 0029: The user can rename any session at once; Core holds the name until the backend can take it

- Status: accepted
- Date: 2026-11-07
- Supersedes: 0026
- Related briefs: PROTO-CORE-API, FLOW-ORGANIZE-SESSIONS, PROTO-CLAUDE-CODE-STREAM, FLOW-FORK-SESSION

## Context

ADR 0026 keeps a session's title the backend's: Ersa stores what finds a session again and the user's arrangement of it, never its history or its title. A rename goes through the backend. But a backend can only rename a session it has recorded, and Claude Code records a session, and reports its id, only when the first turn opens; a fork has no record until its first message. So a rename in that window would be refused. The owner decided rename should work immediately. The backend keeps the user's name apart from the name it generates (for Claude Code, a `custom-title` entry beside its `ai-title`), and the user's name shows in place of the generated one.

## Options considered

1. **Rename unavailable until the backend records the session**: no title in Ersa, but Rename is offered and withheld by a state the user can't see, and a fork withholds it until its first message.
2. **Core holds the user's name until the backend can take it**: rename always works; Core keeps a copy only until it has written the name to the backend.
3. **Ersa keeps every title itself**: rename always works, but the name would not reach the backend's own tools, against ADR 0017's ordinary sessions.

## Decision

Option 2, and ADR 0026's decision otherwise stands, restated here so this record is whole.

A session's history is the backend's own session record, read through the adapter, and details only the live stream carries are shown and not stored; Core's operational records go to an app log; Ersa sessions are ordinary backend sessions, launched the way the vendor's own IDE integration launches them (for Claude Code, the user's settings load, `CLAUDE_CODE_ENTRYPOINT` is `ersa`, and inherited Claude variables are cleared); Ersa lists and resumes any session in the backend's record for a project, and its own sessions can be resumed elsewhere by id; retention is the backend's, except that the user's Delete deletes the record.

Per session, Core stores what finds it again (host, project, workspace, backend, the backend's session id), its mode, and the user's arrangement of it: whether it is pinned, its section, whether it is archived, and the time its latest turn ended. Core stores the user's sections, and the rail's grouping, sort, filter and collapsed groups are user settings in Core.

A session's title is the backend's, with one exception: a name the user gives a session its backend has not recorded yet. Core stores that name at once, reports it as the session's title, and writes it to the backend as the user's name once the backend reports the session's id, then drops its copy.

## Consequences

- ADR 0017's and 0026's context and consequences still hold, among them that a session's view is the record plus the live stream merged on the backend's message ids, that deleting a session in Ersa deletes its record, and that archive is Ersa's.
- The rule becomes: per session, Ersa stores what finds it again and the user's arrangement of it, never its history, and its title only until the backend can take the user's name.
- A rename is never refused because a session is starting (PROTO-CORE-API-R51).
- How a fork is named is decided when forking is built (FLOW-FORK-SESSION, Open questions).
