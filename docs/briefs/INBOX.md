# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-08 PROTO-CLAUDE-CODE-STREAM: Questions does not say how the adapter reads back an answer the brief's rules leave open. Meanwhile: a question whose text has no key in the host's `answers` gets no answer (so `question.answered` can hold fewer than one per question), a key naming no question is ignored, an empty multi-select value "" reads as no options chosen, and where labels that hold ", " let more than one set of options join to the value, the set taking the earliest labels wins.
- [ ] 2026-11-08 PROTO-CLAUDE-CODE-STREAM: Questions says questions sharing one text share one key, but not what `session.answer_question` does when the user gives them different answers, which the CLI cannot carry. Meanwhile: the adapter refuses such answers (`invalid_state`), as it refuses answers that miss a question, repeat one, name an option not offered, or give a list to a single-choice question.
- [ ] 2026-11-08 PROTO-AGENT-EVENTS: R18 rules out `permission.requested` for the question tool but says nothing of the tool's own `tool_use` and `tool_result`, which still project to `tool.requested` and `tool.result`, so a transcript can show the question as a tool call beside its question card. Meanwhile: left as tool events.
- [ ] 2026-11-08 PROTO-CLAUDE-CODE-STREAM: the question fixtures are now committed (2.1.316 `07-question-answer`, `08-question-interrupt`, `09-question-deny`), but were recorded with an isolating invocation (`--setting-sources ""`, `--strict-mcp-config`, no `--include-partial-messages`), not Ersa's own, while the brief's intro says the 2.1.316 set used Ersa's invocation and Questions still calls them not yet in the fixture set. Meanwhile: committed as recorded; the driver's allow is compared to the recording as JSON, since Ersa writes object keys in sorted order.

## Resolved recently (optional, trimmed in planning passes)
