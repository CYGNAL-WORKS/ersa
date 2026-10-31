---
id: FLOW-APPLY-PACK
type: flow
title: Subscribe to, apply, and update a pack
status: draft
references: [LAY-SHELL]
adrs: []
---

# Subscribe to, apply, and update a pack

## Intent

The user adds a pack source (a folder, a network share, or a URL). Ersa lists the packs there with their versions and descriptions. The user applies a pack to a project; Ersa writes the agents, skills, MCP configuration, and prompts into the forms each backend expects and records what it applied. When the source publishes a new version, Ersa shows it and the user updates with one action and can see what changed. A user can also edit a pack locally and publish it back to a source they have write access to.

## Open questions

- Pack manifest format and how it maps one pack onto several backends' configuration layouts.
- Whether applying a pack to a project modifies tracked files in the repository (visible to teammates) or only local configuration.
- Signing or verification of packs from HTTP sources.

## Changes

- 2026-10-31: created as draft.
