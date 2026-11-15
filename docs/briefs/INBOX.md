# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 FLOW-NEW-SESSION: COMPOSER.md's status line has no row or place in its order for a host whose backend listing failed (it defers to R11); built in the place of "a host with no backend": "Ersa couldn't detect the backends on this host." then the reason, with the box unavailable, and the backend chip reads "Detection failed" with the danger glyph. Built so far still says R30 and R11 are unbuilt.
- [ ] 2026-11-14 FLOW-NEW-SESSION: R30 does not say whether a refused change's line survives switching to another session and back, or whether the chips work while Ersa can't reach Core; built: the refusal is kept per session until the user sends or opens a chip there, and the session's chips are unavailable while Core is away, as the box is.




## Resolved recently (optional, trimmed in planning passes)
