# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-01 PROTO-CLAUDE-CODE-STREAM: on 2.1.316 a trusted project's session (`--setting-sources user,project,local`) runs the workspace's hooks and `.mcp.json` servers, but the CLI ignores the workspace's `permissions.allow` with a stderr warning ("this workspace has not been trusted ... set projects[<path>].hasTrustDialogAccepted") because its own trust record for the new worktree is unset; so ADR 0018's trusted project gets hooks and MCP but not its always-allow rules until the CLI's trust record can be set (the brief's open question). Built R12 as written; the brief's Invocation bullet says always-allow rules apply.
- [ ] 2026-11-01 PROTO-CLAUDE-CODE-STREAM: R18's gate (`--setting-sources user` loads no workspace settings, hooks, `.mcp.json`, skills, agents or commands) was observed on 2.1.316 only, while the brief's minimum tested version is 2.1.302 and Core's detection still names 2.1.302 as both minimum and maximum; an opt-in test (`only_a_trusted_projects_workspace_configuration_loads_in_the_real_cli`) re-checks it against whatever CLI is installed. Should the minimum move to the oldest version this was verified on, and should Core's tested range follow the adapter's?

## Resolved recently (optional, trimmed in planning passes)
