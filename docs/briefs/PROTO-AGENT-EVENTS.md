---
id: PROTO-AGENT-EVENTS
type: protocol
title: Agent event protocol
status: draft
references: []
adrs: [0003, 0004]
---

# Agent event protocol

## Intent

Ersa's own vocabulary for what happens in a session, independent of any backend. Adapters emit these events; the event log stores them; the Interface renders them. Commands flow the other way: user input, permission decisions, interrupt, stop. The protocol is defined by a JSON Schema that is the source of truth; Rust and TypeScript types derive from it. Expected event families: session lifecycle, assistant text (streamed), tool use (requested, running, result), permission (requested, decided), cost and context updates, errors, and backend-native passthrough for anything not yet modelled.

## Open questions

- Granularity of streamed text: per token, per chunk, or per block.
- Whether cost and context are events or derived state.
- How to represent backend features Ersa does not model yet without the Interface special-casing backends.
- Versioning: a protocol version field per event, or per session.

## Changes

- 2026-10-31: created as draft.
