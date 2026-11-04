# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-04 PROTO-CLAUDE-CODE-STREAM: the encoded-cwd example names the owner's disk layout (`D:\Projects\ersa` becomes `D--Projects-ersa`); the repository goes public, so the example wants a neutral path such as `C:\Users\user\repo` becomes `C--Users-user-repo`. Code and recordings were scrubbed meanwhile.
- [ ] 2026-11-04 none: `.claude/skills/implement-ticket/SKILL.md` says CI runs on the owner's machine as the owner's user and waits while it is offline; CI now runs on GitHub-hosted runners, so a stalled check is a failed run, not an offline machine. Nothing done meanwhile.
- [ ] 2026-11-04 none: ADR 0022 rests on a private repository with no ruleset or merge queue, and `.github/workflows/review.yml` stops once the repository is public; the owner decided the merge review runs as a local poller on the owner's machine, enqueuing into a merge queue, which needs a superseding ADR and a ticket. Nothing done meanwhile.

## Resolved recently (optional, trimmed in planning passes)
