# Briefs

A brief is the source of truth for one capability of Ersa, written from the user's perspective. Code and tests derive from briefs and cite them. Briefs never cite code. The rules here implement ADR 0013.

## Types

| Type | Describes | ID prefix | Requirement IDs |
|------|-----------|-----------|-----------------|
| product | what Ersa is, for whom, and the principles that resolve trade-offs | `PRD-` | verified by review |
| flow | what a user does across screens and systems to reach an outcome | `FLOW-` | yes, tested |
| layout | which elements a screen or region contains and how they relate | `LAY-` | none until after M1 |
| component | one behavior-heavy interface piece: purpose, behavior, accessibility | `CMP-` | none until after M1 |
| protocol | how two parts of the system talk: messages, schemas, sequencing | `PROTO-` | yes, tested |
| architecture | how the system is built: parts, boundaries, responsibilities | `ARCH-` | verified by review |

Component briefs exist only for components with non-trivial behavior (the shelf, the composer, the tile). Appearance, states, variants, and tokens are owned by the component gallery and `docs/design/DESIGN.md`, not by briefs.

## Referencing

A brief references other briefs by ID in its front matter and in prose. Two rules, both checked by the manifest: no reference cycles, and no restating. If a flow needs to say what the composer looks like, it references `LAY-COMPOSER`; it does not describe it.

## Identity

- Brief IDs are the type prefix plus a short uppercase slug: `LAY-SHELL`, `FLOW-NEW-SESSION`.
- The file is `docs/briefs/<ID>.md`. Recorded protocol fixtures live in `docs/briefs/protocol/fixtures/` (writable by implementing sessions, since they are evidence). JSON Schemas that implement a protocol brief live at the repository root under `protocol/schema/` and are maintained by implementing tickets with conformance tests; the brief defines the vocabulary, the schema is its machine-readable form.
- Requirement IDs are the brief ID plus `-R` and a two-digit-or-more number: `FLOW-NEW-SESSION-R03`. Allocated in order, never reused. The manifest reports duplicates.

## Front matter

```yaml
---
id: FLOW-NEW-SESSION
type: flow
title: Start a new session
status: draft
references: [LAY-SHELL]
adrs: [0011]
---
```

Product and architecture briefs add `verified-by: review`.

`status` is one of:

- `draft`: a paragraph of intent and a list of open questions. Not yet buildable.
- `ready`: fully written, requirements allocated, next to be built.
- `built`: implemented and verified. Changes from here are deliberate.
- `deprecated`: no longer true; kept for its IDs. Says what replaced it.

## Requirements

One testable statement per requirement, written in the form "When X, Ersa shall Y" or "Ersa shall Y" where no trigger applies, as a list item with its ID in bold:

```markdown
- **FLOW-NEW-SESSION-R01**: When the user starts a session for a git project, Ersa shall create a new worktree for it before the backend starts.
```

- Rewording that preserves meaning keeps the ID.
- A change of meaning retires the old ID and allocates a new one. Mark the retired line with strikethrough and say what replaced it: `- ~~**FLOW-NEW-SESSION-R02**~~: retired 2026-11-02, see FLOW-NEW-SESSION-R07.`
- A test cites a requirement with the literal text `verifies: FLOW-NEW-SESSION-R01` in its name or a comment. Only citations in test files count (paths containing `tests/` or `__tests__/`, or names ending in `.test.*`, `.spec.*`, or `_test.rs`).
- A commit that implements requirements ends with an `Implements:` trailer listing their IDs.

## Roles

- **Planning sessions** edit briefs and ADRs. They declare themselves by setting `ERSA_ROLE=planning`; a tool hook blocks edits under `docs/briefs/` and `docs/adr/` otherwise.
- **Implementing sessions** build tickets. When they find a brief contradicting reality, or make a decision the brief does not cover, they append a line to `docs/briefs/INBOX.md` and continue; a gap in `CONTENT.md` or `DESIGN.md` goes there too. The merge gate holds a pull request while its inbox has open entries (ADR 0025), so a planning pass clears them before it merges; CI stays green and the merge review runs meanwhile. The session lists its entries in the pull request description, and the planning pass that resolves them is a commit pushed to that pull request's branch. A harness or process proposal no code depends on is not an entry: it is a follow-up in the pull request.

## Changing a brief

Before writing or rewriting a brief for an area, read the matching file under `docs/reference/lessons/` if one exists; its README says which file covers which area.

- `draft` briefs may be rewritten freely by planning sessions.
- `ready` and `built` briefs change in a planning pass that lists the requirement IDs added, retired, or reworded, and updates the tests that cite retired IDs in the same change.
- Each brief ends with a `## Changes` section, newest first, one line per change. Briefs merge with git's union driver (`.gitattributes`), so two branches that each add a Changes line or a requirement merge by keeping both lines; after such a merge, read the section and run the manifest check, which reports a requirement defined twice.

## Manifest

`node scripts/manifest.mjs` regenerates `docs/MANIFEST.md` and the table in `docs/adr/README.md`. It reports: every brief with status, requirement count, and verified count; unverified requirements in ready or built flow and protocol briefs; open inbox entries; duplicate IDs; reference cycles; dangling references; retired IDs still cited; and layout or component briefs carrying requirement IDs while still draft. Pass `--check` to exit non-zero on problems.
