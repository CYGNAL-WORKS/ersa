---
id: PROTO-COPILOT-SDK
type: protocol
title: Copilot SDK to Ersa event mapping
status: draft
references: [PROTO-AGENT-EVENTS]
adrs: [0009]
---

# Copilot SDK to Ersa event mapping

## Intent

How the Copilot adapter uses the official Copilot Rust SDK and maps its session, message, tool, and permission types onto `PROTO-AGENT-EVENTS`. Covers: SDK and CLI version pinning; starting the CLI in server mode through the transport (local, and over SSH using the SDK's external-server option); the permission handler and how it reaches the shelf; what the SDK exposes for usage so the rail tile can show context and cost; and what raw record is persisted for a Copilot session so the projection rule holds. This brief does not document the wire protocol; the SDK owns it.

## Open questions

- Which SDK version to pin and how CLI protocol-version mismatches surface to the user.
- Mapping Copilot's permission and tool vocabulary onto Ersa's without flattening either.
- Whether the CLI supports an equivalent of system-prompt additions for modes.
- What "raw log" means for an SDK-mediated backend: the SDK's JSON-RPC frames, or its typed events serialized.

## Changes

- 2026-10-31: replaces PROTO-COPILOT-RPC; wire protocol delegated to the official SDK per ADR 0009.
