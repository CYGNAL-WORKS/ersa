# Ersa

Ersa is a desktop harness for running many coding-agent sessions at once, each in an isolated workspace on a local or remote host, driven through the vendor CLIs the user already has (Claude Code first, GitHub Copilot second) via their structured-event protocols. Ersa never holds a model API key and never parses terminal output.

## How we work

- **Briefs are the source of truth for behaviour.** They live in `docs/briefs/`. Code and tests derive from briefs and cite them by requirement ID. Briefs never cite files, functions, or code. Read `docs/briefs/README.md` before writing or changing one.
- **Decisions live in `docs/adr/`.** Before proposing a change of direction, read the ADR index. Record a decision when it would be costly to reverse or when a future reader could plausibly choose differently. Supersede, never edit.
- **Words live in `docs/GLOSSARY.md`.** Use the glossary term everywhere: briefs, identifiers, UI copy. A new concept gets a term before it is used: planning sessions add it, implementing sessions propose it as an inbox line.
- **Roles.** Planning sessions set `ERSA_ROLE=planning` and may edit briefs, ADRs, the glossary, `CLAUDE.md` and `.claude/`. Implementing sessions build tickets, leave those planning-only paths alone (a hook enforces this for edit tools), and record anything a brief gets wrong or leaves open as a line in `docs/briefs/INBOX.md`, then continue.
- **Tickets are GitHub issues** (ADR 0016), cut from briefs with the `to-tickets` skill and built by implementing sessions with the `implement-ticket` skill (orchestration procedure: `docs/agents/orchestration.md`); tracker conventions are in `docs/agents/issue-tracker.md`, labels in `docs/agents/triage-labels.md`. A pull request that completes a ticket says `Fixes #n`. Working plans live in `.scratch/`, uncommitted. Commits that implement requirements end with an `Implements:` trailer listing the IDs.
- **Requirement IDs are stable.** Flow and protocol briefs carry them, like `FLOW-NEW-SESSION-R03`. Tests cite them with `verifies: <ID>`; only citations in test files count. Run `node scripts/manifest.mjs --check` before committing; it regenerates `docs/MANIFEST.md` and the ADR index and fails on open inbox entries and structural problems.

## Design

Ersa must be beautiful, and every session that touches the interface is responsible for that, not a later polish pass. Before writing or changing interface code, read `docs/design/DESIGN.md`. The rules that apply to every change:

- Ersa is an instrument, not a chat app. Quiet warm-neutral surfaces, one accent colour, colour only where it means something.
- Agent output reads as a document, never as speech bubbles. Status lives in tiles with one shared anatomy.
- Every colour, size, radius, and duration comes from a token in `DESIGN.md`. A raw value in interface code is a defect.
- Every transition uses a motion token and explains where something came from or went to. Nothing animates for decoration.
- Light and dark are designed together. A component is not done until it is right in both.
- Nothing that signals "AI": no gradients, no glow, no sparkle iconography, no purple. If it would look at home on a model vendor's landing page, choose differently.
- Hover, focus, pressed, disabled, loading, empty, and error states each get a deliberate appearance, and the rail's attention state is an edge or glyph, never a filled row.

## Stack

Rust core running as its own process; Tauri 2 shell; TypeScript interface speaking two schema-defined protocols to the core. Windows first, with nothing in the core that would block macOS. The architecture is `docs/briefs/ARCH-CORE.md`; the reasons are in the ADRs.

## Writing for agents

Keep every document short enough to read in one sitting. Put detail behind a pointer whose wording says when to follow it. State what to do rather than what to avoid. When a document grows past about two pages, split it.
