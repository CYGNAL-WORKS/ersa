# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-27 FLOW-CLEAN-UP: a `branch` item reported `merged` that still holds `unshared` commits (a squash-merged branch) is refused by `cleanup.remove` without `discard`, since its commits go, though R16 ticks it as one whose changes the base branch already holds. #545 ticks it, sends `discard` for it, counts none of its commits as lost in the question, and keeps the confirming button's plain wording for it; whether R17 should count those commits, or the button read as discarding, is open.
