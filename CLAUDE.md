# Ersa

Ersa is a desktop harness for running many agentic coding sessions at once. Each session runs in its own isolated workspace, on the local machine or a remote one, and is driven through a vendor CLI the user already has installed (Claude Code first, GitHub Copilot second). Ersa never holds or sends model API keys.

## How we work

- **Briefs are the source of truth for behaviour.** They live in `docs/briefs/`. Code and tests derive from briefs and cite them by requirement ID. Briefs never cite files, functions, or code. Read `docs/briefs/README.md` before writing or changing one.
- **Decisions live in `docs/adr/`.** Before proposing a change of direction, read the ADR index. Record a decision when it would be costly to reverse or when a future reader could plausibly choose differently. Supersede, never edit.
- **Words live in `docs/GLOSSARY.md`.** Use the glossary term everywhere: briefs, identifiers, UI copy. If a concept has no term, add one before using it.
- **Plans and tickets are ephemeral.** They live in `.scratch/`, which is not committed, and die when the work merges. Any decision made while implementing is written back to a brief or ADR before the ticket closes.
- **Roles.** Planning sessions edit briefs and ADRs. Implementing sessions build tickets and do not edit briefs; on finding a contradiction between a brief and reality, stop and report it.
- **Requirement IDs are stable.** A requirement looks like `LAY-SHELL-R03`. Tests cite it with the literal text `verifies: LAY-SHELL-R03`. Rewording a requirement keeps its ID; changing its meaning retires the ID and issues a new one. Run `node scripts/manifest.mjs` to regenerate `docs/MANIFEST.md`.

## Stack

Rust core, Tauri 2 shell, TypeScript UI. Windows first, with nothing in the core that would block macOS. The architecture is in `docs/briefs/ARCH-CORE.md`; the reasons are in the ADRs.

## Writing for agents

Keep every document short enough to read in one sitting. Put detail behind a pointer whose wording says when to follow it. State what to do rather than what to avoid. When a document grows past about two pages, split it.
