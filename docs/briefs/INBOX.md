# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-12 PROTO-CORE-API: for a backend whose titles may span lines (R70), `session.set_title` says only "takes each run of spacing as one space, keeping line breaks"; it leaves open whether each line is trimmed, what becomes of blank lines, and whether a kept line break counts toward the limit. #294 trims each line, takes each run of spacing within a line as one space, keeps interior blank lines, drops blank lines at either end, treats LF and CRLF as line breaks (a lone CR, U+0085, U+2028 and U+2029 count as spacing), and counts each kept line break as a character. No registered adapter has multi-line titles yet.

## Resolved recently (optional, trimmed in planning passes)
