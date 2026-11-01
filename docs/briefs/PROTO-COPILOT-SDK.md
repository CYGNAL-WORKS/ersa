---
id: PROTO-COPILOT-SDK
type: protocol
title: Copilot SDK to Ersa event mapping
status: draft
references: [PROTO-AGENT-EVENTS]
adrs: [0009, 0017]
---

# Copilot SDK to Ersa event mapping

## Intent

How the Copilot adapter uses the official Copilot Rust SDK and maps its session, message, tool, and permission types onto `PROTO-AGENT-EVENTS`. Covers: SDK and CLI version pinning; starting the CLI in server mode through the transport (local, and over SSH using the SDK's external-server option); the permission handler and how it reaches the shelf; what the SDK exposes for usage so the rail tile can show context and cost; and history and listing through the SDK (ADR 0017). A Copilot session's history is the CLI's own session record (under `~/.copilot/session-state/<id>/`), read only through the SDK: `list_sessions` lists sessions, `resume_session` resumes one by id, and `Session::get_events` returns its history as the same event type as the live stream, minus ephemeral events such as deltas, so one projection serves both inputs. Only Copilot CLI sessions are in scope, not VS Code chat sessions. This brief does not document the wire protocol; the SDK owns it.

## Open questions

- Which SDK version to pin and how CLI protocol-version mismatches surface to the user.
- Mapping Copilot's permission and tool vocabulary onto Ersa's without flattening either.
- Whether the CLI supports an equivalent of system-prompt additions for modes.
- Which SDK event field is the message id on which history and live events are merged.

## Changes

- 2026-11-01: history and listing through the SDK (`list_sessions`, `resume_session`, `Session::get_events`), Copilot CLI sessions only, per ADR 0017; the raw-log open question replaced.
- 2026-10-31: replaces PROTO-COPILOT-RPC; wire protocol delegated to the official SDK per ADR 0009.
