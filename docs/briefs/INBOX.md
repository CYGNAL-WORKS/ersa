# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-06 LAY-SHELL: The rail holds a merge's leftovers that contradict each other: two opening lines, an "Add project sits at the end of the tree" bullet, and three versions each of Loading, empty and failed and of The foot (two under The shelf), whose empty-tree notes differ ("how to add one", "to start with New session", "to choose a folder in New session"). #156 built the tree to the issue and the 2026-11-05 change log: no Add project row in the tree, and the empty tree says to start with New session; the existing add-project form stays beneath the tree until the project-chip ticket.

## Resolved recently (optional, trimmed in planning passes)
