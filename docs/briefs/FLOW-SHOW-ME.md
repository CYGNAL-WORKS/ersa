---
id: FLOW-SHOW-ME
type: flow
title: Ask the agent to show me
status: draft
references: [LAY-SHELL, PROTO-HARNESS-TOOLS]
adrs: []
---

# Ask the agent to show me

## Intent

The user asks how to do something in Ersa ("how do I add a remote host?"). The agent, using the interface-driving tools, asks for consent, then performs the steps visibly: opening panes, highlighting controls, filling fields, pausing to explain. The user can stop at any point and take over. Nothing irreversible happens without a confirmation from the user. The same mechanism powers first-run onboarding.

## Open questions

- Which actions are demonstrable (navigation, highlighting, filling) versus forbidden (deleting, sending, applying).
- How the agent learns the interface's current structure: a static map, or a live accessibility tree.
- Whether "show me" is a dedicated session or available in any session.

## Changes

- 2026-10-31: created as draft.
