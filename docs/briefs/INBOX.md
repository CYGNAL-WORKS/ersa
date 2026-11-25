# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-24 PROTO-CLAUDE-CODE-STREAM: the opening paragraph calls each stream `.jsonl` "verbatim CLI output", but every recording passes through the redactor, which since #471 also replaces the plan's name (`subscriptionType`), each rate-limit window's `utilization` (now 0.5 in every window, so no recording shows two windows with different shares) and the ids in a synced plugin's path, besides the email, organization and paths it already replaced. Meanwhile the fixtures are redacted and the brief is unchanged; it could say "the CLI's output with the login's details replaced by placeholders".


