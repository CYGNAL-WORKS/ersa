# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-01 PROTO-CLAUDE-CODE-STREAM: with `--setting-sources user,project,local` and no `--strict-mcp-config` (R12), a workspace's own `.claude/settings.json` and `.mcp.json` now load in headless mode, where the CLI shows no workspace-trust prompt (seen on 2.1.316: `claude --resume` in a new Ersa worktree asks "Is this a project you created or one you trust?", Ersa's launch never does), so opening a session on an untrusted clone runs its hooks and MCP servers and its `permissions.allow`/`defaultMode` can pre-approve tools or override the `--permission-mode` Ersa shows; no brief or ADR says whether Ersa asks for workspace trust first or shows the effective mode. Built as ADR 0017 says, no trust step.
- [ ] 2026-11-01 ARCH-CORE: R13 needs a transport to remove variables from the environment a process inherits, not only set them; the local transports now take a list of names to remove (applied before the ones set), and no brief says a future remote (SSH) transport must honour removals too.
- [ ] 2026-11-01 PROTO-CLAUDE-CODE-STREAM: R13's removal list is a fixed denylist of five names, but a Core started from inside Claude Desktop inherits about thirty `CLAUDE_CODE_*` and `CLAUDE_*` variables (seen: `CLAUDE_CODE_SIMPLE`, `CLAUDE_CODE_SESSION_ID`, `CLAUDE_CODE_HOST_SESSION_ID`, `CLAUDE_CODE_DISABLE_CRON`, `CLAUDE_CODE_MESSAGING_SOCKET`/`_TOKEN`, `CLAUDE_CODE_OAUTH_SCOPES`, `CLAUDE_AGENT_SDK_VERSION`), a VS Code terminal adds `CLAUDE_CODE_SSE_PORT` and `ENABLE_IDE_INTEGRATION`, and settings files can set variables again through their `env` key; all of these still reach the session. Built the five R13 names; should the adapter remove every inherited `CLAUDE_*` variable (keeping user-set ones such as `CLAUDE_CONFIG_DIR`?) or allowlist?

## Resolved recently (optional, trimmed in planning passes)
