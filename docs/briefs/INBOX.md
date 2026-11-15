# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 PROTO-CLAUDE-CODE-STREAM: fixture 2.1.316/54 (a message of words, an image and a file taken in part way through a tool turn) shows the `remove` entry's `content` is the message's text blocks joined by newlines, so its file list sits in the words and its image is gone, while the `queued_command` attachment entry just before it, which is on the conversation's chain (`uuid`, `parentUuid`), holds the blocks as sent in `attachment.prompt` and the echo's id in `attachment.source_uuid`. R69's "holding its `content`" would show the list's tags as words and drop the image; meanwhile the reader takes the message's blocks from that attachment when the record has it, and from `content` otherwise (fixture 52's attachment is elided), at the `remove`'s place.





## Resolved recently (optional, trimmed in planning passes)
