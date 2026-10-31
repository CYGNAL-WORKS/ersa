---
id: PROTO-CORE-API
type: protocol
title: Core request/response API
status: draft
references: [PROTO-AGENT-EVENTS]
adrs: [0011]
---

# Core request/response API

## Intent

The request/response protocol between the Interface and the Core process over a local socket: everything that is not a session's event stream. Projects, hosts, workspaces, and sessions (list, create, update, remove); starting, stopping, and resuming sessions; sending user input and permission decisions; reading files and diffs for the workbench; settings; pack and mode operations. Defined by a JSON Schema from which TypeScript types are generated. Versioned so a shell can refuse to attach to an incompatible Core.

## Open questions

- Framing: JSON-RPC 2.0 over newline-delimited JSON, or a simpler request-id envelope.
- Whether file and diff reads stream (large files) or page.
- Attach semantics: how a shell discovers a running Core and whether more than one shell may attach.

## Changes

- 2026-10-31: created as draft.
