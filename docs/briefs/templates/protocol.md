---
id: PROTO-EXAMPLE
type: protocol
title: Noun phrase naming the protocol
status: draft
references: []
adrs: []
---

# Title

## Parties

Who talks to whom, and which side initiates.

## Transport and framing

How bytes move (stdio, socket, in-process channel) and how messages are delimited (newline-delimited JSON, length-prefixed, JSON-RPC).

## Messages

One subsection per message type: name, direction, purpose, fields. The authoritative shape is the JSON Schema in `schema/`; this section explains it.

### message-name

Direction. Purpose. Notable fields and their meaning.

## Sequencing

A Mermaid `sequenceDiagram` for the common exchange, then the rules: what must precede what, what may interleave, what ends an exchange.

## Errors and recovery

How errors are expressed, what the receiver does, how a broken stream is detected and resumed.

## Versioning

How a party declares its version and how incompatibilities are handled.

## Conformance

What a conforming implementation must pass. Recorded fixtures live beside the schema.

## Requirements

- **PROTO-EXAMPLE-R01**: ...

## Out of scope

## Open questions

## Changes

- YYYY-MM-DD: created.
