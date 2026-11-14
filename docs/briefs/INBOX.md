# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 PROTO-CORE-API: Errors says `session_not_running` is "the session has no live backend", which literally covers a send to a session whose backend failed to start, but `session.send` says that refusal carries the adapter's reason so the new-session screen can show it (FLOW-NEW-SESSION-R08), and R80 would replace it with the catalog's words. Meanwhile #325 gives the failed-start refusal ("the backend could not be started: ...") no `reason`; other methods on such a session (rename, interrupt) that find the backend gone answer `session_not_running`.
- [ ] 2026-11-14 PROTO-CORE-API: Errors says `open_elsewhere` names the other process "in `detail`" without a form. Meanwhile #325 sets `detail` to the pids alone, comma-separated (`4242, 4343`), so the Interface can name them in its own words; `message` keeps Core's English.


## Resolved recently (optional, trimmed in planning passes)
