# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-19 FLOW-APPROVE-PERMISSION: R19 covers only Core refusing an answer; no requirement says what the shelf and the transcript's card show for an answer sent that got no answer (a timeout, or the connection closed after it was queued), when Core may have applied it, and docs/design/RAIL.md's shelf notes don't list it. Built meanwhile (#377), as FLOW-SEND-MESSAGE-R15 does for a message: it is never said to be refused; the answers are offered again with a note, "Ersa can't tell whether your answer reached the agent. Answer again if this request is still waiting.", until the request's decision arrives or the user answers again. An answer that never reached Core still shows as not accepted, with why.


## Resolved recently (optional, trimmed in planning passes)
