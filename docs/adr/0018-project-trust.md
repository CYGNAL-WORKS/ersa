# 0018: The user trusts a project once, when adding it; only a trusted project's own configuration loads

- Status: accepted
- Date: 2026-11-01
- Related briefs: PROTO-CORE-API, PROTO-CLAUDE-CODE-STREAM, FLOW-NEW-SESSION

## Context

ADR 0017 launches Claude Code sessions with the user's, the project's and the local settings and the user's MCP configuration, as the vendor's IDE integration does. A repository's own `.claude/settings.json` and `.mcp.json` can run hooks and MCP servers, pre-approve tools (`permissions.allow`) and change the permission mode. In a terminal the CLI asks whether the user trusts a folder before loading them; headless sessions never ask (observed on 2.1.316). Every session runs in a new worktree, which the CLI has never seen, so its own trust record cannot be relied on.

## Options considered

1. **No trust step**: simplest. Opening a session on a cloned repository runs whatever that repository configures.
2. **Read the CLI's own trust record**: consistent with the terminal, but every new worktree is untrusted there, so in practice nothing would load.
3. **Ersa asks once per project and remembers**, as VS Code's workspace trust does: one decision per repository, and it covers every worktree made from it.

## Decision

Option 3. When the user adds a project, Ersa asks whether they trust it and remembers the answer; the user can change it later. A trusted project's sessions load the project's and the local settings and the project's MCP configuration as ADR 0017 says. An untrusted project's sessions load only the user's own settings and none of the repository's configuration. Where a backend keeps its own trust record and offers a safe way to set it, Ersa marks a trusted project's workspaces trusted there too, so resuming the session in the backend's own tools does not ask again.

## Consequences

Adding a project gains one question. A repository's configuration never runs before the user has said they trust it. Whether the Claude Code CLI's trust record can be set safely from outside (it lives in the CLI's own configuration file, which running CLI processes also write) is open; until it can, resuming an Ersa session in a terminal asks for trust there once per worktree.
