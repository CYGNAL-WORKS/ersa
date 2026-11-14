# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 PROTO-CORE-API: R90 has a send still waiting its turn report `checking`, but `session.list`'s `sending` says "absent unless a send is under way"; meanwhile a send waiting its turn reports `checking` on `session.send_progress` and is absent from `sending` until it has its session's slot, so an Interface that reconnects then learns of it from the next report (at most 5 seconds). Should `sending` show a send waiting its turn?
- [ ] 2026-11-14 PROTO-CORE-API: `session.send_progress` "at most once a second" is per send's reporting; where one send hands the session's reports to the next (a waiting send getting its slot, or a queued send taking over when the one ahead ends) two reports can fall within a second, the first being the same `checking`. Meanwhile kept: a busy connection coalesces them, and the hand-off between queued sends reports within 250 ms of the earlier send ending rather than waiting out the second.


## Resolved recently (optional, trimmed in planning passes)
