# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 PROTO-CLAUDE-CODE-STREAM: Open question on `get_context_usage` answered by 2.1.316 fixtures `44-context-usage`, `45-context-usage-resumed` and `46-context-usage-during-compact`. The answer carries `totalTokens`, `maxTokens`, `rawMaxTokens` and also `model` (the model its figures are for), `percentage`, `autocompactSource`, `autoCompactThreshold`, `isAutoCompactEnabled`, `apiUsage` (the last API call's counts, null before any call and after a compaction), `categories`, and lists of the machine's own agents, skills, MCP tools and memory files (the recorder elides those). It answers before the first turn (32,983 tokens: system prompt, tools, agents, skills). For a session just resumed it answers before any message with the whole resumed conversation (44,974). The adapter reports what the answers give; the brief can drop the question.
- [ ] 2026-11-15 PROTO-CLAUDE-CODE-STREAM: `totalTokens` is the sum of the answer's "used" categories, an estimate, not an API count. After a turn it equals the last call's input + cache-creation + cache-read tokens, output excluded (44,225 = 2 + 12,922 + 31,301), while R71 adds output tokens on each `assistant` line, so the ring moves by the output count as the two sources alternate. Built as R71 says; the brief may want to say so or drop output from R71's sum.
- [ ] 2026-11-15 PROTO-CLAUDE-CODE-STREAM: Open question on whether `result.modelUsage` and `get_context_usage` give different windows answered for the pinned CLI: both give 1,000,000 for `claude-opus-5-5` and `claude-haiku-5-5` (fixture `44-context-usage`). R74 is built as written.
- [ ] 2026-11-15 PROTO-CLAUDE-CODE-STREAM: R72's request after a `compact_boundary` goes out while the compaction turn still runs; the CLI answers it just after that turn's `result`, with the compacted count (45,402 in fixture `46-context-usage-during-compact`). R74's "the session's current model" is built as the model the stream last reported (`system/init`, `set_model`'s answer), and an answer's window counts for the model the answer names, else that model. "After each `set_model` it answers" is built as a success answer: a refused `set_model` changes no model, so nothing is asked.




## Resolved recently (optional, trimmed in planning passes)
