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

The user asks how to do something in Ersa ("how do I add a remote host?"). The agent asks for consent; a visible overlay marks that the interface is being driven. The agent then demonstrates with three tools: focus a pane or region, highlight a control, and explain in a short message. It does not fill fields or press controls in the first version. The user can stop at any point. Permission controls are unreachable by these tools regardless of consent. Guided first-run onboarding built on this is a later addition.

## Open questions

- How the agent learns the interface's current structure: a static identifier map or a live tree.
- Whether "show me" is available in any session or in a dedicated help session.
- Whether filling fields is ever added, and under what confirmation.

## Changes

- 2026-10-31: reduced to focus, highlight, explain; consent overlay; permission controls forbidden; onboarding deferred.
- 2026-10-31: created as draft.
