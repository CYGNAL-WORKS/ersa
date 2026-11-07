# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-07 FLOW-SEND-MESSAGE-R11: no design doc says how a message's attachments look in the transcript. #193 shows them in the bubble above the words: an image as itself, at most `size-attachment-image` (a pending token, 224px) either way, with a `line` edge and `radius-md`; a file or folder as the composer's attachment without its detail or remove, its name the path's last part, the path the agent was given on hover. DESIGN.md (Layout, the user's bubble) or COMPOSER.md can say so and take the token.

## Resolved recently (optional, trimmed in planning passes)
