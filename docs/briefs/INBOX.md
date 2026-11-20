# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-20 FLOW-SEND-MESSAGE-R23 (#378 merge review): COMPOSER.md's status line puts a failed dialog before an unknown send, a refused message and a refused change as a fixed order, but the reason given is that it shows before an older line, and a refusal that arrives after the dialog failed is newer. As written, that later refusal would be hidden and never announced. Built meanwhile: the dialog line shows before the lines already there when it failed; one that arrives later shows instead, and the dialog line shows again once that one goes, until R23's clearing actions.


## Resolved recently (optional, trimmed in planning passes)
