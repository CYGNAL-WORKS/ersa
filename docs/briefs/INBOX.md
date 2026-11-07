# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-06 PROTO-CORE-API: `session.resume` on an archived session "clears `archived` first", but resume itself is still milestone M1 and answers `invalid_state`; the brief does not say what a resume before M1 does to an archived session. Core clears the mark and then gives the M1 answer, so FLOW-ARCHIVE-DELETE-SESSION step 2's Resume unarchives the session and reports that resuming is not built yet; the next message is then refused as not running.

## Resolved recently (optional, trimmed in planning passes)
