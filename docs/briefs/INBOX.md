# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 none (COMPOSER.md, The context ring): the reset wording is observed in two forms only ("Resets Wed 2:00 PM", "Resets in 1 hr 15 min"). Built meanwhile (#422): a weekday and time when 24 hours or more away (a reset over a week away reads the same way), else the time left as "Resets in 1 hr 15 min", "Resets in 2 hr" or "Resets in 15 min", minutes rounded up, and a time already past reads "Resets in 1 min"; the time left is counted when the list opens.
- [ ] 2026-11-15 LAY-SHELL (Context): the pages leave open how the ring's list looks and whether it is a tab stop. Built meanwhile (#422): the session controls' ring is a tab stop on every session and on the new-session screen (one line there, the context); with keyboard focus its pill edge turns `ink` as a chip's does, and it shows a tooltip above it, each line the reading's name as the ring says it ("5-hour limit: 84 percent used") with when it resets at the trailing edge in `ink-muted`. The nearest limit is compared on the whole percent each reading shows, so a context reading 85 percent beats a window at 84.9.





## Resolved recently (optional, trimmed in planning passes)
