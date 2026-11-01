# 0012: Claude Code is driven in streaming JSON mode with explicit flags, permissions through the harness MCP server

- Status: superseded by 0014
- Date: 2026-10-31
- Related briefs: PROTO-CLAUDE-CODE-STREAM, PROTO-HARNESS-TOOLS

## Context

Claude Code's structured-event interface is its print mode with streaming JSON input and output, which runs as a long-lived bidirectional session when fed stream-json input. Ersa drives the binary the user has already logged into, so usage follows that login; Ersa holds no API key (ADR 0004). Review raised two risks: the CLI's defaults for print mode are documented to change (a bare mode that skips login, project configuration, hooks, and MCP configuration is announced to become the default), and the stdin control channel used for permission prompts is documented primarily through the vendor's Agent SDK, while the documented CLI path for permission prompts is an MCP tool named on the command line.

## Options considered

1. **Rely on defaults and the control channel as observed**: least code; breaks silently on a CLI update.
2. **Pass every behaviour-affecting flag explicitly, feature-detect from the CLI's initial system message, and route permission prompts through Ersa's own MCP server using the documented permission-prompt-tool mechanism**: more setup, robust to default changes, and reuses a server Ersa builds anyway.

## Decision

Option 2. The Claude Code adapter never depends on a default: mode flags, permission mode, MCP configuration, system-prompt additions, and session identifiers are always passed. Capabilities are read from the CLI's initial message rather than inferred from version strings. Permission requests arrive as calls to a tool on the harness MCP server and are answered from the interface. A maximum tested CLI version is recorded; above it the adapter warns and continues. The adapter records usage reported by the CLI so any change in how the login is metered is visible in the first week of use.

## Consequences

The harness MCP server moves from milestone 6 to milestone 0, in a minimal form (the permission tool only). The protocol brief documents the exact flags and messages observed, with fixtures. An interactive-terminal fallback (driving the TUI through a pseudo-terminal and reading the CLI's own session files) is explicitly not pursued: Ersa needs structured events, not console output.
