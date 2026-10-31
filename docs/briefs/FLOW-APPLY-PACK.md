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

The user adds a pack source (a folder, a network share, or a URL). Ersa lists the packs there with versions and descriptions. The user applies a pack to a project; Ersa renders the pack's neutral definition into the native format of each backend installed for that project (for Claude Code, its plugin and settings layout; for Copilot, its equivalents) and records what it applied. When the source publishes a new version, Ersa shows it, and the user updates with one action and can see what changed. A user with write access can edit a pack locally and publish it back to the source.

## Open questions

- The neutral pack schema, and which concepts (agents, skills, tool servers, prompts, hooks) it covers in the first version.
- Whether applying a pack modifies tracked files in the repository (visible to teammates) or only local configuration.
- Verification of packs from HTTP sources.
- How a pack expresses something only one backend supports.

## Changes

- 2026-10-31: pack is a neutral definition rendered per backend.
- 2026-10-31: created as draft.
