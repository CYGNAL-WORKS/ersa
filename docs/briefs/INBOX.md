# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 FLOW-READ-TRANSCRIPT: Built so far still says diff cards (R09, R13) and the stopped state (R16) are to build; #333 builds them. Meanwhile nothing changed in the brief.
- [ ] 2026-11-14 FLOW-READ-TRANSCRIPT: a turn that ends with a call's permission request still open leaves that request answerable (only `session.ended` closes one), while R16 shows its card as stopped. Meanwhile the card says Stopped, which wins over Waiting for you, and the permission card is unchanged; decide whether `turn.completed` closes an open request too.
- [ ] 2026-11-14 none (docs/design/TRANSCRIPT.md, Diff cards): the page says what an open diff card holds only as the change, and not how a call that changes several files tells its diffs apart. Meanwhile it shows no input fields (the change is the input), a failed call's Error beneath the change as a tool card shows it, and, with several files, each file's path above its diff in `mono` at `ui-small` size in `ink-muted` under a `line` hairline.


## Resolved recently (optional, trimmed in planning passes)
