# 0007: Remote sessions run the backend CLI on the remote host

- Status: accepted
- Date: 2026-10-31
- Related briefs: ARCH-CORE

## Context

Users want sessions on other machines, VMs, and containers with the same isolation as local ones. Two shapes are possible: run the backend CLI remotely and stream its protocol over SSH, or run the CLI locally against a remotely mounted filesystem.

## Options considered

1. **Remote CLI over SSH**: the agent runs next to the code it edits; Ersa streams structured events over the connection. Needs the backend installed and authenticated on the remote host.
2. **Local CLI, remote filesystem**: one installation to manage. Rejected: remote filesystems are slow and semantically leaky for agents that run builds and tests, and tool execution would happen on the wrong machine.

## Decision

Option 1. A transport abstraction (local spawn, SSH exec, container exec) starts the backend process on the chosen host and carries its stdio. Adapters are transport-agnostic.

## Consequences

Each host needs the backend CLI and its credentials, which is the user's responsibility and matches how Claude Desktop handles remote sessions. Workspace isolation (worktrees) must also happen on the remote host, so the VCS layer executes through the same transport. Latency and disconnects become first-class states in the session lifecycle.
