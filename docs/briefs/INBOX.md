# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-13 FLOW-FORK-SESSION-R03: the menu can't show Fork unavailable ahead of a click for most of R03's reasons, so #203's Done-when is only partly met. `session.list` says nothing about how the record's conversation ends or about other processes, `last_activity_at` is no sign of a finished reply (sessions whose turns ended before Core stored it have none), and Core's fork refusals carry no `reason` yet (PROTO-CORE-API-R79 unbuilt). Built meanwhile (#203): Fork is unavailable with "Available once the reply finishes" while the listing shows a reply running, a request waiting on the user or a send on its way, and "Forking…" while a fork of it is on its way; no reply yet, an interrupted last turn, open elsewhere and a backend that can't fork are said after the click, as "Ersa can't fork this session." with Core's message beneath the tile, and Fork stays available so the user can try again once that has changed. To decide: whether `session.list` should carry the session's fork availability (or the menu ask Core as it opens), and the catalog words per `reason` once R79 is built.

## Resolved recently (optional, trimmed in planning passes)
