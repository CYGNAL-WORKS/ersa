# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-03 none: shepherd jobs (scripts/shepherd-job.mjs) still load the pull request's own project settings (env, apiKeyHelper, permissions) and its CLAUDE.md and skills; hooks and MCP servers are now off, and `--setting-sources user` would also drop the review-pr skill the review needs. Decide whether jobs should run from trusted settings and skills instead; meanwhile jobs refuse fork pull requests.


## Resolved recently (optional, trimmed in planning passes)
