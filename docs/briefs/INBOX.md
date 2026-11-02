# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-02 PROTO-CLAUDE-CODE-STREAM: renaming a session Ersa does not have live appends `custom-title` even when the live-session registry says another process (a terminal, VS Code) has it open. The 2.1.316 CLI's own schema says such a process picks up a name another process wrote only when it next reads its record's tail (after about 32 KB of its own writes, or at a compaction), so until then it keeps showing, and may write again, its old name. Meanwhile Ersa appends regardless; a rule is needed (append anyway, refuse, or append and say so with a `session.notice`).
- [ ] 2026-11-02 PROTO-CLAUDE-CODE-STREAM: `rename_session` is now recorded (2.1.316 fixture `03-session-rename`): request `{subtype: "rename_session", title, source?: "remote" | "host", session_id?}`, answered by a bare `{subtype: "success", request_id}` after a new `system/session_title_changed` line (`title`), and the CLI then writes the `custom-title` entry itself. Ersa sends `source: "host"` (the CLI counts it as the user's rename) and the session id (a CLI that moved to another session refuses). The open question on its shapes can close; `session_title_changed` projects as a passthrough meanwhile.
- [ ] 2026-11-02 PROTO-CORE-API: `session.set_title` is silent on a blank title and on a session its backend has not named yet (no backend session id, so no record). Meanwhile Core trims the title, takes the title with each run of spaces as one and refuses as invalid params one that is blank, longer than 200 characters or holds a control character (the CLI lists one line of at most 200, spaced so); the limit is Claude Code's, held in Core until the brief says whether it belongs to the adapter, and answers `invalid_state` for an unnamed session or one whose record is gone.


## Resolved recently (optional, trimmed in planning passes)
