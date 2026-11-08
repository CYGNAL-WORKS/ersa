# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-07 FLOW-ARCHIVE-DELETE-SESSION: neither COMPOSER.md nor RAIL.md says how the archived note looks or where a refused Archive, Unarchive or Resume says why. Built meanwhile (#198): the note sits where the composer does, at its measure, as one row on `canvas` with a `line` hairline edge and `radius-md`, padded `space-sm`: an archive icon at `size-icon-sm` in `ink-muted`, the words in `ui` and `ink-muted`, then Resume as the primary button, reading "Resuming…" and unavailable while Core answers; a refused Resume says why above it, where the status line sits, as a danger problem line; a refused Archive or Unarchive says why beneath the tile, as a refused trust does beneath its heading.

## Resolved recently (optional, trimmed in planning passes)
