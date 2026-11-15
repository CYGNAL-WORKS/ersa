# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 none (docs/design/COMPOSER.md, The composer: the words): it says the field is "three lines tall on the new-session screen"; #411 makes it one line there as in a session, growing with the words. Built per #411 (the composer's three-line mode is retired).
- [ ] 2026-11-15 none (docs/design/COMPOSER.md, The session controls: a chip): it describes "A chip with a prefix, such as Effort"; #411 drops the effort chip's prefix, and nothing else used one, so the chip's prefix is retired. Built per #411.
- [ ] 2026-11-15 none (docs/design/COMPOSER.md, The session controls: Effort): it says Effort with none chosen "reads Default after its prefix"; #411 shows the level alone ("Medium") and "Default" with none, the accessible name still "Effort: <value>". Built per #411.
- [ ] 2026-11-15 FLOW-NEW-SESSION-R33 / COMPOSER.md: no brief says how the backend's default model reads; #411 makes it "Default (<label of the listed model, not marked default, with the same resolved id>)", else "Default (<resolved id>)", else "Default". Built from the string catalog in the Interface for every backend, in the chip and in its list's default row alike (one choice, one label; the issue names only the chip); backend.list still carries the CLI's own label (PROTO-CORE-API-R72). Needs requirement IDs for the tests in SessionControls.test.ts.
- [ ] 2026-11-15 PROTO-CLAUDE-CODE-STREAM (Invocation): the Claude Code adapter's permission mode `default` now reads "Ask" (was "Ask before acting"), as Claude Desktop names it (#411); "Accept edits", "Plan", "Auto" unchanged. No brief named the old label; the adapter test asserts all four. Needs a requirement ID.
- [ ] 2026-11-15 none: #411's one-line box, effort chip and model chip have no requirement IDs, so their tests cite none yet; a planning pass allocates them and the tests take `verifies:` lines.





## Resolved recently (optional, trimmed in planning passes)
