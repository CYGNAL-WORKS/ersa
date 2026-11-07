# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-07 PROTO-CORE-API (attachment.inspect), FLOW-SEND-MESSAGE-R14: the composer can't tell from a path's text whether sending copies it. Core judges the canonical path, through links, junctions and `..`, and a link's raw `target` can be relative. So `attachment.inspect` now takes an optional `workspace_id` and, with it, answers `copied` (boolean, absent for an image, which goes inline), judged by the same rule `session.send` uses. The composer shows the "will be copied" note from that field alone, and `target` still means where a link points. The brief can add both fields to attachment.inspect.

## Resolved recently (optional, trimmed in planning passes)
