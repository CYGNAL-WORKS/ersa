# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-02 LAY-SHELL: "the backend chip appears only when the host has more than one backend" hides a host's lone backend when it is missing, against FLOW-NEW-SESSION-R07; the chip also shows when no backend is available, with the danger glyph and the backend's status in its list (#53).
- [ ] 2026-11-02 PROTO-CORE-API: `backend.list` gives a backend's `name` (`claude-code`) but no label to show; the Interface maps the names it knows ("Claude Code", "GitHub Copilot") and shows any other as named (#53).
- [ ] 2026-11-02 none (DESIGN.md): Elevation allows a soft, short shadow on transient layers but has no token for one, and the new-session wordmark has no display size; the session controls' open list is drawn flat with a line, and the wordmark uses the heading size, muted, meanwhile; there is also no layering token (the open list uses z-index 1) and no size token for small marks, so the ring and the chosen-option dot borrow spacing tokens (#53).

## Resolved recently (optional, trimmed in planning passes)
