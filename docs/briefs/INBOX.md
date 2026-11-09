# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-08 PROTO-CLAUDE-CODE-STREAM: Invocation says `--session-id` goes "never with `--resume`" and that a fork's id is the `session_id` of its `system/init`, and R28 says to store the id the new process reports. The adapter forks with `--resume <id> --fork-session --session-id <new>`: fixture 2.1.316/16 shows the CLI takes it and reports that id, so Core stores the id before the CLI starts, as for a new session, and a fork can be renamed before its first message. Closes FLOW-FORK-SESSION's open question on renaming and PROTO-CORE-API's `session.set_title` carve-out for a fork; R28 and the two Invocation lines need rewording.
- [ ] 2026-11-08 FLOW-FORK-SESSION: Open question on what a fork is named. The CLI's copy keeps the original's `custom-title` unmarked (fixture 2.1.316/19), so it is not "the original's name marked as a fork", and an untitled original's copy announces nothing (fixture 14). Core shows the original's title until the fork's record gives one; no suffix is added. Decide whether Ersa should mark it.
- [ ] 2026-11-08 PROTO-AGENT-EVENTS: A fork's backend never reports a turn until its first message, so it would stay `starting` while FLOW-FORK-SESSION says the composer is ready. Core moves a fork to `idle` once the CLI answers `initialize` (a new lifecycle fact). The brief says `session.state` comes from what Core knows of the process, but names no such transition; add it.
- [ ] 2026-11-08 PROTO-CLAUDE-CODE-STREAM: The CLI writes a fork's record only with its first message (fixture 2.1.316/15, as for a new session). Core keeps the original's id and the fork's last message with the fork (ARCH-CORE's store gains two columns), and shows a fork with no record of its own from the original's record up to that message, so one closed before its first message keeps its conversation. `--resume` can't find such a fork, so `session.resume` (M1) has to start it again from its original; the brief should say so.
- [ ] 2026-11-08 PROTO-CORE-API: `session.fork`'s refusals don't list one for a fork already starting, and `session.send` names no refusal while its session is being forked. Core refuses both with `invalid_state` until the fork's backend is ready: a turn started before the CLI has read the record would be copied part way (fixture 2.1.316/18), and each hung read stays one per session. Add them to the brief.

## Resolved recently (optional, trimmed in planning passes)
