# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-05 none: #162 renders screenshot baselines on GitHub, but no skill tells agents how to update them, so a session may reach for Docker (which the owner does not want started on their machine) or fail CI to get images. Add to `.claude/skills/implement-ticket/SKILL.md`, after the bullet on missing tokens: "- When a change moves the gallery's screenshots, push the branch, then run `node scripts/render-baselines.mjs`: it renders the baselines on GitHub in CI's container and copies them into `apps/shell/e2e/__screenshots__/`. Open the changed images, then commit them. Docker and failed CI runs are the owner's routes, not a session's." Meanwhile the README and the Playwright config say so.

## Resolved recently (optional, trimmed in planning passes)
