# 0014: Speak Claude Code's stdio control protocol directly from Rust, using the Agent SDK types as the reference

- Status: proposed
- Date: 2026-10-31
- Supersedes: 0012
- Related briefs: PROTO-CLAUDE-CODE-STREAM, PROTO-HARNESS-TOOLS, ARCH-CORE

## Context

Recording the real CLI (2.1.302, fixtures 00 to 11) established:

- In print mode with stream-json input and output, the CLI runs a long-lived bidirectional session. Permission prompts reach the host as `control_request` / `can_use_tool` messages only when the host has sent an `initialize` control request and the CLI was started with `--permission-prompt-tool stdio`. Without both, anything needing approval is denied with a `permission_denied` system message.
- The documented MCP route (`--permission-prompt-tool mcp__<server>__<tool>`) also works and carries the same payload, at the cost of running an MCP server.
- The Claude Agent SDK (`@anthropic-ai/claude-agent-sdk`, 0.3.316 at the time of writing) is a TypeScript and Python client that spawns the same binary and speaks the same protocol. Its type definitions describe every stream message and all forty-odd control request types, including `initialize`'s `sdkMcpServers`, which lets the host serve MCP tools in-process over the control channel. Its license is proprietary; it ships no Rust.

ADR 0012 chose the MCP route for permissions because the control channel looked SDK-private. The recordings show the control channel is the primary, typed interface and the MCP route is the fallback.

## Options considered

1. **Use the Agent SDK** via a Node sidecar per session. Typed, maintained, but adds a runtime Ersa chose not to ship, a process hop per session, and a dependency Ersa cannot vendor or inspect in Rust.
2. **Permissions through an MCP server** (ADR 0012). Works, but needs a server process or socket, a per-session token, and a second protocol for what the control channel already carries.
3. **Implement the control protocol subset in Rust**, with the SDK's type definitions as the pinned schema reference and recorded fixtures as the conformance tests.

## Decision

Option 3. The Claude Code adapter starts the CLI with `-p --input-format stream-json --output-format stream-json --verbose --permission-prompt-tool stdio` plus explicit permission mode, setting sources, and MCP configuration, sends `initialize` before the first user message, and handles `can_use_tool` requests by routing them to the shelf. The adapter implements this subset of control requests: `initialize`, `can_use_tool` (inbound), `interrupt`, `set_permission_mode`, `set_model`, `get_context_usage`, `get_session_cost`, and `mcp_message` for in-process harness tools declared through `sdkMcpServers`. The SDK package version whose types were read is recorded in the protocol brief; its definitions are kept in a local scratch copy for reference and not vendored. Each CLI release that changes a used message is a fixture re-recording, not a guess.

## Consequences

No MCP server, socket, or token is needed for permissions or for the harness tools server; both ride the adapter's stdio. The harness tools server becomes a per-session in-process MCP endpoint answered over `mcp_message`, which also makes the "which session is calling" question trivial. The MCP route remains recorded (fixture 10) as a fallback if the control channel changes. Ersa depends on an interface the vendor documents primarily through SDK types; the mitigation is the fixture suite and a pinned maximum tested CLI version. Anthropic's terms page says Agent SDK developers should use API keys; the direct protocol is mechanically identical to the SDK, so the owner's position that driving the user's own installed CLI under the user's own login is permitted use applies equally to both.
