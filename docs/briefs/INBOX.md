# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-03 none: DESIGN.md has no token for a transient layer's shadow, a stacking order, or the small marks (#60). Proposed, built meanwhile from `apps/shell/src/theme/tokens.pending.yaml` (merged by the token generator, which refuses a pending name DESIGN.md already has): `size.glyph: 8px` (the state glyph and the inline problem dot, were `space-sm`), `size.option-mark: 4px` (a chip's chosen-option dot, was `space-xs`), `size.ring: 16px` (the context ring, was `space-lg`); a custom `layer` group with `popover: 10` (the open chip list, was a raw `z-index: 1`); a themed custom `shadow` group with `layer`, light `0 4px 12px -4px color-mix(in oklab, {colors.ink} 14%, transparent)`, dark `0 4px 12px -4px color-mix(in oklab, {colors.canvas} 70%, transparent)` (now on the open chip list). Elevation & Depth could name `shadow-layer` and `layer-*`, Layout the mark sizes. The wordmark size the issue asked for is already in DESIGN.md. Moving them means adding them to the front matter and deleting the pending file.

## Resolved recently (optional, trimmed in planning passes)
