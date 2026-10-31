# Briefs

A brief is the source of truth for one capability of Ersa, written from the user's perspective. Code and tests derive from briefs and cite them. Briefs never cite code.

## Types

| Type | Describes | ID prefix | References |
|------|-----------|-----------|------------|
| product | what Ersa is, for whom, and the principles that resolve trade-offs | `PRD-` | flows |
| flow | what a user does across screens and systems to reach an outcome | `FLOW-` | layouts, protocols |
| layout | which elements a screen or region contains and how they relate spatially | `LAY-` | components, layouts |
| component | one reusable interface piece: purpose, anatomy, states, behaviour | `CMP-` | components |
| protocol | how two parts of the system talk: messages, schemas, sequencing | `PROTO-` | protocols |
| architecture | how the system is built: parts, boundaries, responsibilities | `ARCH-` | protocols |

Layering rule: a brief references a lower layer by ID and never restates its content. If a flow needs to say what the composer looks like, it references `LAY-COMPOSER`; it does not describe it.

## Identity

- Brief IDs are the type prefix plus a short uppercase slug: `LAY-SHELL`, `CMP-PERMISSION-TILE`, `FLOW-NEW-SESSION`.
- The file is `docs/briefs/<ID>.md`. Protocol briefs may carry schemas in `docs/briefs/protocol/schema/`.
- Requirement IDs are the brief ID plus `-R` and a two-digit-or-more number: `LAY-SHELL-R03`. They are allocated in order and never reused.

## Front matter

```yaml
---
id: LAY-SHELL
type: layout
title: Application shell
status: draft
references: [CMP-RAIL, LAY-TRANSCRIPT]
adrs: [0003]
---
```

`status` is one of:

- `draft`: a paragraph of intent and a list of open questions. Not yet buildable.
- `ready`: fully written, requirements allocated, next to be built.
- `built`: implemented and verified. Changes from here are deliberate.
- `deprecated`: no longer true; kept for its IDs. Say what replaced it.

## Requirements

Write each requirement as one testable statement, as a list item with its ID in bold:

```markdown
- **LAY-SHELL-R01**: The rail, transcript, composer, and workbench are visible at the default window size without scrolling the shell itself.
```

Rules:

- Rewording that preserves meaning keeps the ID.
- A change of meaning retires the old ID and allocates a new one. Mark the retired line with strikethrough and say what replaced it: `- ~~**LAY-SHELL-R02**~~: retired 2026-11-02, see LAY-SHELL-R07.`
- Tests cite a requirement with the literal text `verifies: LAY-SHELL-R01` in a comment or test name. The manifest counts these.

## What goes in, what stays out

In: behaviour, constraints, states, error cases, out-of-scope statements, open questions, user stories where they clarify intent.

Out: file paths, function names, library names, code (except schemas in protocol briefs), implementation plans, and anything that belongs in an ADR (the why) or the glossary (the words).

## Changing a brief

- `draft` briefs may be rewritten freely.
- `ready` and `built` briefs change in a deliberate planning pass. The change lists which requirement IDs were added, retired, or reworded, and the tests that cite retired IDs are updated in the same change.
- Implementing sessions do not edit briefs. On finding a contradiction, they stop and report it.
- Each brief ends with a `## Changes` section, newest first, one line per change.

## Manifest

`node scripts/manifest.mjs` regenerates `docs/MANIFEST.md`: every brief, its status, requirement count, how many requirements have a citing test, dangling references, and retired IDs still cited. Pass `--check` to exit non-zero on problems.
