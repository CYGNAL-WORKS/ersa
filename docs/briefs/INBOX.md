# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-06 none (DESIGN.md): the workspace list's branches scroll after eight rows, and no size token says how tall that is; added `size-list-max` (224px, eight options of `size-control`) in tokens.pending.yaml for a planning pass to move into DESIGN.md.
- [ ] 2026-11-06 FLOW-NEW-SESSION-R21 (COMPOSER.md, The workspace list): nothing says what the base branch section shows while the branches are read or when Core cannot list them; it says "Reading the branches…" or "Ersa couldn't read the branches: " and Core's reason in the no-match note's place, the chip reads "New workspace", and the workspace starts at Core's default.
- [ ] 2026-11-06 none (CONTENT.md): Project folder's status, "Shares files with you" (COMPOSER.md, The workspace list), breaks the rule that a Status is two words at most; the catalog key is named a Description meanwhile, so the rule does not apply to it.

## Resolved recently (optional, trimmed in planning passes)
