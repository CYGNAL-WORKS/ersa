# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-07 PROTO-CLAUDE-CODE-STREAM: Renaming says a rename before the first message is "to be recorded as a fixture"; it is now recorded as 2.1.316/09-session-id-rename (`--session-id`, `initialize`, `rename_session` before the first message, then one message; the record holds the `custom-title`), made with the session recorder's new `--session-id` option. The brief's text was left as it is.
- [ ] 2026-11-07 PROTO-CLAUDE-CODE-STREAM: the 2.1.316 CLI's own schema describes `system/session_title_changed` as the session's name, sent after each change to it and never for an AI-generated title, and says a name another session already has can get a suffix that makes it unique; the brief says neither. Meanwhile Core shows the name the CLI announced over the one `session.set_title` asked for, so a suffixed name is the one `session.list` lists.

## Resolved recently (optional, trimmed in planning passes)
