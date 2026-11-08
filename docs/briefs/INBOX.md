# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-08 none (DESIGN.md, Motion): a breathing mark "softens" its tone, but no token says how far. Defined `opacity.breath: 0.4` in `apps/shell/src/theme/tokens.pending.yaml` (a new custom `opacity` group, emitted as `--opacity-<name>`), used by the working glyph's loop and the needs-you chevron's one breath (from full tone to `opacity-breath` and back). Proposed for DESIGN.md.
## Resolved recently (optional, trimmed in planning passes)
