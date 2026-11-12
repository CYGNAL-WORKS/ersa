# Ersa

Ersa is a desktop harness for running many coding-agent sessions at once, each in an isolated workspace on a local or remote host, driven through the vendor CLIs the user already has (Claude Code first, GitHub Copilot second) via their structured-event protocols. Ersa never holds a model API key and never parses terminal output.

## How we work

- **The backend owns what it owns.** Ersa abstracts what the backend CLIs do into one common shape; it never adds to or duplicates their concepts. If the backend owns it, Ersa asks the backend.
- **Ersa is built agentically.** Agents specify, implement, verify and review; the owner directs and makes the judgment calls. Verification is the bottleneck, so prefer a check a machine runs over a person reading a diff. When an agent gets something wrong, fix the harness (brief, skill, check, hook) as well as the code, so the next session gets it right. Auto-memory is off for this project: a lesson kept in one agent's memory is lost to every other agent, so it goes in the harness.
- **Every line is a liability.** Code, briefs and skills are cheap to generate and expensive to keep, so the usual failure is organic growth: bolting more onto an interface without asking whether its shape still fits. Look for something to retire before you add. When drift already causes friction, propose the reshape (a follow-up or a ticket) instead of patching around it again, even when it is not the smallest local change. A pull request that adds surface and removes none says what it keeps and why.
- **Check assumptions before building; Docket reviews the diff.** Whatever the session's role, before building anything that rests on a reading of a brief or on how a peer behaves, write the plan with its assumptions and have the `assumption-checker` agent try to disprove them in a fresh context (`implement-ticket` step 2). Then open the pull request: Docket runs the merge review on every head whose CI passed, with the reviewers `.claude/review.json` names and the `REVIEW.md` rubric, and a clean head merges. Ask Docket for a pull request's state (`implement-ticket` step 5), never GitHub (ADR 0030).
- **Decide the details; escalate only what the owner must see.** Settle a product question yourself: do what Claude Desktop does, and where it is silent, what serves the people `PRD-ERSA` names best, the non-technical ones included. Record the call where the work lands (a brief's Changes line, the pull request) so the owner can audit it. Take a choice to the owner only when it departs from Claude Desktop or from `PRD-ERSA`'s principles, is costly to reverse, or changes a decision the owner made.
- **Escalations are in plain English.** Anything handed to the owner to decide leads with plain English: what goes wrong, for whom, why it matters, and what to decide. Technical detail follows it and never replaces it.
- **Briefs are the source of truth for behavior.** They live in `docs/briefs/`. Code and tests derive from briefs and cite them by requirement ID. Briefs never cite files, functions, or code. Read `docs/briefs/README.md` before writing or changing one.
- **Decisions live in `docs/adr/`.** Before proposing a change of direction, read the ADR index. Record a decision when it would be costly to reverse or when a future reader could plausibly choose differently. Supersede, never edit.
- **Words live in `docs/GLOSSARY.md`.** Use the glossary term everywhere: briefs, identifiers, UI copy. A new concept gets a term before it is used: planning sessions add it, implementing sessions propose it as an inbox line.
- **Roles.** Planning sessions set `ERSA_ROLE=planning` and may edit briefs, ADRs, the glossary, `CLAUDE.md`, `REVIEW.md` and `.claude/`. Implementing sessions build tickets, leave those planning-only paths alone (a hook enforces this for edit tools), and record anything a brief gets wrong or leaves open as a line in `docs/briefs/INBOX.md`, then continue; the merge gate holds the pull request until a planning pass closes it. A proposal no code depends on is not an inbox line: propose it in chat when the owner is present, else as a follow-up in the pull request.
- **Tickets are GitHub issues** (ADR 0016), cut from briefs with the `to-tickets` skill and built by implementing sessions with the `implement-ticket` skill (orchestration procedure: `docs/agents/orchestration.md`); tracker conventions are in `docs/agents/issue-tracker.md`, labels in `docs/agents/triage-labels.md`. A pull request that completes a ticket says `Fixes #n`. Working plans live in `.scratch/`, uncommitted. Commits that implement requirements end with an `Implements:` trailer listing the IDs.
- **Requirement IDs are stable.** Flow and protocol briefs carry them, like `FLOW-NEW-SESSION-R03`. Tests cite them with `verifies: <ID>`; only citations in test files count. Run `node scripts/manifest.mjs --check` before committing (a hook runs it on an agent's `git commit` and blocks on a problem); it regenerates `docs/MANIFEST.md` and the ADR index and fails on structural problems.

## Design

Ersa must be beautiful, and every session that touches the interface is responsible for that, not a later polish pass. Before writing or changing interface code, read `docs/design/DESIGN.md`. The rules that apply to every change:

- Ersa looks and behaves like the agentic harnesses people already use, and is more beautiful than they are. Quiet neutral surfaces, one accent color, color only where it means something.
- The user's messages sit in chat bubbles; the agent's output has no bubble and reads as a document. Status lives in tiles with one shared anatomy.
- Every color, size, radius, and duration comes from a token in `DESIGN.md`. A raw value in interface code is a defect.
- Every transition uses a motion token. Motion is calm, never delays input, and honors reduced motion.
- Light and dark are designed together. A component is not done until it is right in both.
- Calm and luminous: soft tonal gradients for material, a faint wash on the canvas, and one focal accent glow per screen. Nothing busy.
- Hover, focus, pressed, disabled, loading, empty, and error states each get a deliberate appearance, and the rail's attention state is an edge or glyph, never a filled row.

## Content

Every word Ersa shows and every document here follows the rules `docs/design/CONTENT.md` gives its kind: US English, rules taken from the Microsoft Writing Style Guide and checked by Vale in CI. Read it before writing interface copy or user-facing docs. Interface copy lives in the string catalog, never in a component.

## Stack

Rust core running as its own process; Tauri 2 shell; TypeScript interface speaking two schema-defined protocols to the core. Windows first, with nothing in the core that would block macOS. The architecture is `docs/briefs/ARCH-CORE.md`; the reasons are in the ADRs.

## Writing for agents

Keep every document short enough to read in one sitting. Put detail behind a pointer whose wording says when to follow it. State what to do rather than what to avoid. Describe what is now: the history of a change belongs in the ADR or brief change log that made it, a retired requirement keeps its struck-through line so its number stays reserved, and a pull request body covers only what its own diff changes. When a document grows past about two pages, split it.
