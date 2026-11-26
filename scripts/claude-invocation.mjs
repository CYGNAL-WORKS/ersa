// Ersa's command line and environment for a Claude Code session
// (PROTO-CLAUDE-CODE-STREAM, Invocation), for the recorders that start the
// CLI as Ersa does. One copy, so a recording never runs a stale command line.

export const ERSA_ARGS = [
  "--input-format", "stream-json",
  "--output-format", "stream-json",
  "--verbose",
  "--include-partial-messages",
  "--permission-mode", "default",
  "--setting-sources", "user,project,local",
  "--permission-prompt-tool", "stdio",
  "--await-initialize",
  "--replay-user-messages",
];
// Ersa's environment changes: its own entrypoint, and nothing inherited from
// whatever started it.
export const ENV_SET = { CLAUDE_CODE_ENTRYPOINT: "ersa" };
// R13 and R19: what a parent Claude Code session would otherwise hand down
// (its session, messaging socket, IDE link).
export const ENV_REMOVED = [
  "CLAUDECODE",
  "CLAUDE_CODE_CHILD_SESSION",
  "CLAUDE_CODE_ENTRYPOINT",
  "NODE_OPTIONS",
  "TRACEPARENT",
  "TRACESTATE",
  "CLAUDE_CODE_SESSION_ID",
  "CLAUDE_CODE_HOST_SESSION_ID",
  "CLAUDE_CODE_MESSAGING_SOCKET",
  "CLAUDE_CODE_MESSAGING_TOKEN",
  "CLAUDE_CODE_SSE_PORT",
  "ENABLE_IDE_INTEGRATION",
  "CLAUDE_AGENT_SDK_VERSION",
];
