---
id: PROTO-COPILOT-RPC
type: protocol
title: Copilot CLI JSON-RPC protocol
status: draft
references: [PROTO-AGENT-EVENTS]
adrs: [0003, 0004]
---

# Copilot CLI JSON-RPC protocol

## Intent

The protocol the GitHub Copilot CLI speaks in its server mode, as wrapped by the official SDKs: how a session is created, how prompts are sent, how streamed responses and tool events arrive, how permissions are negotiated, and how the connection is closed. Derived from the SDKs' source and from observation, with recorded fixtures. Written so the Copilot adapter can be implemented in Rust without an SDK.

## Open questions

- Which SDK version's protocol to pin to, and how the CLI signals protocol changes.
- Mapping of Copilot's permission and tool model onto `PROTO-AGENT-EVENTS`.
- Whether the CLI supports an equivalent of system-prompt injection for modes.

## Changes

- 2026-10-31: created as draft.
