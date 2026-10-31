---
id: FLOW-EXAMPLE
type: flow
title: Verb phrase naming the outcome, e.g. Start a new session
status: draft
references: []
adrs: []
---

# Title

## Outcome

What the user has when the flow ends, in one sentence.

## Trigger

Where the flow starts and what the user does to begin it.

## Steps

Numbered. Each step names the layout the user is looking at by ID and what they see and do. State changes the user can observe go here; internal mechanics do not.

1. On `LAY-...`, the user ...
2. ...

## Variations and errors

What happens when things go differently: cancellations, failures, retries, offline, permission denied. One short paragraph or list per case.

## States

If the flow has meaningful intermediate states, a Mermaid `stateDiagram-v2` here. Keep it to the states a user can tell apart.

## Requirements

- **FLOW-EXAMPLE-R01**: ...

## Out of scope

## Open questions

## Changes

- YYYY-MM-DD: created.
