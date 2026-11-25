# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-24 none (docs/design/COMPOSER.md, A chip's list): the page says a list floats beneath its chip, or above it where the window has no room, but not where it goes sideways. With each mode's description, the permission mode chip's list is wider than the room from that chip, the row's last, to the window's edge, and with only an up-or-down flip it covered its own chip. #588 lets every chip's list, the attach menu included, end where its chip ends when it can't start where its chip starts, as Claude Desktop's permission menu sits above its chip at the row's end; a list that fits is placed as before.
