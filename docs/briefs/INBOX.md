# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 PROTO-CLAUDE-CODE-STREAM: Naming a new session still cites the raw capture `2.1.316/session-title-2026-11-13.txt`; #280 recorded it as the numbered fixture `2.1.316/43-session-title` (the answer arrives before the CLI's `--replay-user-messages` echo of the first message, and the record gets the `ai-title` entry). Meanwhile the tests use fixture 43.
- [ ] 2026-11-14 PROTO-CLAUDE-CODE-STREAM-R49: "a message of attachments alone waits for the next" is open when the session is relaunched before a message with words: a relaunch after any message resumes the session (`--resume`), and R49 sends nothing for a resumed session, so that session is never named. Meanwhile the adapter asks only in the process that started the new session; a relaunch before any message (no record yet) starts it under its id again and asks there.
- [ ] 2026-11-14 PROTO-CORE-API-R83: a title change Core learns from the record (the summary read on resume, reopening or a session's end, say a newer last prompt or an `ai-title` written by another process) is not among R83's three causes, so Core sends no tile for it and a window shows the old title until it reads `session.list`. Meanwhile Core sends tiles only for `session.set_title`, an announced rename and a generated name.



## Resolved recently (optional, trimmed in planning passes)
