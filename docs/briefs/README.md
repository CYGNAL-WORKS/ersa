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

- The ID in a line's leading bold defines the requirement. An ID written in full anywhere else, in a requirement or in prose, refers to one.
- Rewording that preserves meaning keeps the ID.
- A change of meaning retires the old ID and allocates a new one. Mark the retired line with strikethrough and say what replaced it: `- ~~**FLOW-NEW-SESSION-R02**~~: retired 2026-11-02, see FLOW-NEW-SESSION-R07.`
- A test cites a requirement with the literal text `verifies: FLOW-NEW-SESSION-R01` in its name or a comment. Only citations in test files count (paths containing `tests/` or `__tests__/`, or names ending in `.test.*`, `.spec.*`, or `_test.rs`); for a requirement cited only elsewhere, the manifest names the file.
- A commit that implements requirements ends with an `Implements:` trailer listing their IDs.

## Roles

Who edits a brief, and what an implementing session does when a brief is wrong or silent, is `CLAUDE.md`'s Roles rule; `docs/briefs/INBOX.md` says how to write an entry. The planning pass that resolves an entry is a commit on that pull request's branch (`docs/agents/orchestration.md`, Resolve inbox entries).

## Changing a brief

Before writing or rewriting a brief for an area, read the matching file under `docs/reference/lessons/` if one exists; its README says which file covers which area.

- `draft` briefs may be rewritten freely by planning sessions.
- `ready` and `built` briefs change in a planning pass that lists the requirement IDs added, retired, or reworded. In the same change, each test that cites a retired ID is cited to the new ID when what it asserts is true under the new wording, and loses its citation otherwise, so the new ID shows as unverified until its ticket builds it.
- A planning pass searches the briefs and design pages for each rule it changes and changes every statement of that rule in the same commit, then has the `assumption-checker` agent try to disprove its plan before pushing, as `implement-ticket` step 2 does. A rule a test could check carries a requirement ID, never only prose or a Changes line.
- A section that mirrors a behavior of a vendor's CLI or app (a prompt's words, a dialog's answers and their order) cites the recording or screenshot under `docs/briefs/protocol/fixtures/` that shows it, and says no more than the vendor does. The recording comes first: the manifest fails a brief that cites a fixture nobody recorded. Ersa looks like the harnesses people already use; a brief that invents words for a behavior the vendor already has is redone when the owner sees it.
- A `## Built so far` section is one paragraph on one line, and says only what `main` has or what the pull request it rides on builds: it is written in the planning commit on that ticket's branch, so it lands with the code, never in a pull request that merges ahead of the code it describes. The manifest fails a brief whose Built so far holds two lines, which is what a union merge of two planning commits that each rewrote it leaves.
- Each brief ends with a `## Changes` section, newest first, one line per change. Briefs merge with git's union driver (`.gitattributes`), so two branches that each add a Changes line or a requirement merge by keeping both lines; after such a merge, read the section and run the manifest check, which reports a requirement defined twice, a section that appears twice, and a bullet label (bold, or a method name in code) repeated under one section name. A rewritten bullet or paragraph survives such a merge as both its old and new wording, and the check sees only bullets whose label stayed the same; keep the version the brief's newest change says.

## Manifest

`node scripts/manifest.mjs` regenerates `docs/MANIFEST.md` and the table in `docs/adr/README.md`. It reports: every brief with status, requirement count, and verified count; unverified requirements in ready or built flow and protocol briefs; open inbox entries, and inbox list items that are not one; duplicate IDs; reference cycles; dangling references; cited fixtures that do not exist; retired IDs still cited; and layout or component briefs carrying requirement IDs while still draft. Pass `--check` to exit non-zero on problems. Open inbox entries count as a problem only with `--inbox` too, which CI passes on the merge queue and on main (ADR 0025).
