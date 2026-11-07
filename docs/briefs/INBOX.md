# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-07 PROTO-CLAUDE-CODE-STREAM-R26, R27 (User message): the recorded message with attachments (fixture 2.1.316/07) settles what the brief left open. The CLI keeps the image and list blocks verbatim in the record, and writes beside them an `isMeta` user entry `[Image: source: <path of its cached copy>]`, which the adapter projects as a system `user.message` like any other text the CLI wrote; and for a prompt of attachments alone it leaves `last-prompt` at the prompt before. The brief can say both, and whether that `[Image: source: …]` entry should be shown at all.

## Resolved recently (optional, trimmed in planning passes)
