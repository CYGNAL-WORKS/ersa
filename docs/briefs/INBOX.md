# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-08 PROTO-CLAUDE-CODE-STREAM: Invocation says `--session-id` goes "never with `--resume`" and that a fork's id is the `session_id` of its `system/init`, and R28 says to store the id the new process reports. The adapter forks with `--resume <id> --fork-session --session-id <new>`: fixture 2.1.316/16 shows the CLI takes it and reports that id, so Core stores the id before the CLI starts, as for a new session, and a fork can be renamed before its first message. Closes FLOW-FORK-SESSION's open question on renaming and PROTO-CORE-API's `session.set_title` carve-out for a fork; R28 and the two Invocation lines need rewording.
- [ ] 2026-11-08 FLOW-FORK-SESSION: Open question on what a fork is named. The CLI's copy keeps the original's `custom-title` unmarked (fixture 2.1.316/19), so it is not "the original's name marked as a fork", and an untitled original's copy announces nothing (fixture 14). Core shows the original's title until the fork's record gives one; no suffix is added. Decide whether Ersa should mark it.
- [ ] 2026-11-08 PROTO-AGENT-EVENTS: A fork's backend never reports a turn until its first message, so it would stay `starting` while FLOW-FORK-SESSION says the composer is ready. Core moves a fork to `idle` once the CLI answers `initialize` (a new lifecycle fact). The brief says `session.state` comes from what Core knows of the process, but names no such transition; add it.
- [ ] 2026-11-08 PROTO-CLAUDE-CODE-STREAM: The CLI writes a fork's record only with its first message (fixture 2.1.316/15, as for a new session). A fork stopped before its first message reads as empty once its view closes, and `--resume` can't find it. `session.resume` (M1) has to start such a fork again from its original, or Core has to keep the original's id with the fork; nothing does yet, since resume is not built.

## Resolved recently (optional, trimmed in planning passes)
