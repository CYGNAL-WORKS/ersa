---
id: PROTO-AGENT-EVENTS
type: protocol
title: Agent event protocol
status: draft
references: []
adrs: [0004, 0010, 0011]
---

# Agent event protocol

## Intent

Ersa's own vocabulary for what happens in a session, independent of any backend. It is a versioned projection of the raw backend log (ADR 0010): adapters provide the function from raw lines to these events; the Core streams them to the Interface per session; the Interface renders them. Expected event families: session lifecycle, assistant text (streamed), tool use (requested, running, result), permission (requested, decided), question (asked, answered), usage (context consumed, cost as reported), errors, and a typed passthrough for backend features not yet modelled. Defined by a JSON Schema that is the source of truth; Rust and TypeScript types derive from it. Commands in the other direction (user input, permission decisions, interrupt, stop) belong to `PROTO-CORE-API`.

## Open questions

- Granularity of streamed text: per chunk or per block.
- How the projection version is carried: per stream, per event, or negotiated at attach.
- How to represent backend features Ersa does not model yet without the Interface special-casing backends.

## Changes

- 2026-10-31: reframed as a projection of the raw log; commands moved to PROTO-CORE-API.
- 2026-10-31: created as draft.
