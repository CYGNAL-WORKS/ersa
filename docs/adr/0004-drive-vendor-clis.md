# 0004: Drive vendor CLIs rather than model APIs

- Status: accepted
- Date: 2026-10-31
- Related briefs: ARCH-CORE, PROTO-AGENT-EVENTS

## Context

Users already have Claude Code and GitHub Copilot installed and authenticated, often under subscription terms rather than API billing. Ersa must not hold API keys or create a second billing relationship. Each vendor CLI exposes a machine-readable mode: Claude Code has a streaming JSON output format with a bidirectional control channel; Copilot exposes a JSON-RPC server mode that its SDKs wrap.

## Options considered

1. **Call model APIs directly**: full control of the agent loop. Rejected: requires keys, duplicates what the vendor harnesses already do well, and loses vendor features (permissions, hooks, skills, MCP) that we would have to rebuild.
2. **Drive the CLIs through a pseudo-terminal and scrape output**: maximum fidelity to the vendor's own UI. Rejected: fragile, terminal-looking, and loses structured events.
3. **Drive the CLIs through their machine-readable protocols**: structured events in, structured commands out; we own rendering, the permissions UI, and persistence.

## Decision

Option 3. One adapter per backend translates the vendor protocol into Ersa's agent event protocol. Documenting the Claude Code streaming protocol against the real CLI is a first-milestone deliverable, because the vendor's own documentation of it is thin. Users install nothing beyond the vendor CLI itself.

## Consequences

Ersa owns the transcript renderer, permission tiles, and cost and context display. Vendor protocol changes can break adapters, so each adapter has conformance tests against recorded fixtures and a documented minimum CLI version. Ersa can never do anything a vendor CLI cannot do; features like modes are implemented through what the CLI exposes (system-prompt injection, settings, MCP configuration), never by patching the vendor.
