---
id: PRD-ERSA
type: product
title: Ersa
status: ready
references: [FLOW-NEW-SESSION, FLOW-APPROVE-PERMISSION, FLOW-RESUME-SESSION, FLOW-REMOTE-SESSION, FLOW-APPLY-PACK, FLOW-SHOW-ME]
adrs: [0002, 0003, 0004, 0006, 0007]
---

# Ersa

## Problem

People who work with coding agents end up running several at once, each in its own terminal window, each on its own branch, with no shared view of what is happening, what needs their attention, and what it is costing. Switching between vendors means switching tools entirely. Sharing good agent setups (skills, prompts, MCP servers) with a team means teaching everyone git. Non-developers who could benefit from agents are put off by tools that reach for code first.

## Who it is for

- A developer running two to ten agent sessions in parallel across one or more repositories, who needs to see at a glance which ones are waiting on them.
- The same developer working on a remote machine, VM, or container, who wants the same experience as local.
- A team lead who wants to share a curated set of agents, skills, and tools with the team without a git tutorial.
- A content or design person using agents for writing, scripts, and assets, who needs the tool to start from their work, not from source code.

## Solution

Ersa is a desktop application that hosts many agent sessions in one window. Each session runs a vendor's own CLI (Claude Code first, GitHub Copilot second) in its own isolated workspace, locally or on a remote host, and Ersa renders the conversation as a rich transcript with permission, cost, and context tiles. The window has four regions: a rail listing projects, workspaces, and sessions; the transcript; a composer; and a workbench of purpose-built panes (files, diff, terminal, browser). Packs let teams share agent setups from a simple versioned source. Modes change the base instructions so the agent reaches for the right kind of tool first. A "show me" mode lets the agent drive the interface to teach the user.

## Principles

- **The user's existing tools, the user's existing auth.** Ersa drives CLIs the user already has. It never asks for or stores a model API key.
- **Many sessions, one glance.** Anything that needs the user's attention is visible from the rail without opening the session.
- **Isolation by default.** A session cannot accidentally work in another session's checkout.
- **Local and remote feel the same.** A remote session has the same features, tiles, and panes as a local one; only latency differs.
- **Beautiful and calm.** The interface is an instrument, not a chat toy. See `docs/design/DESIGN.md`.
- **Dogfood first.** The first usable version is the one its author switches to daily; everything else waits.

## Flows

- `FLOW-NEW-SESSION`: add a project, create a workspace, start a session with a chosen backend and mode.
- `FLOW-APPROVE-PERMISSION`: see a pending permission from the rail, open it, approve or deny, with cost and context visible.
- `FLOW-RESUME-SESSION`: close Ersa, reopen it, and continue every session where it left off.
- `FLOW-REMOTE-SESSION`: add a remote host, create a workspace there, run a session on it.
- `FLOW-APPLY-PACK`: subscribe to a pack source, apply a pack to a project, receive an update.
- `FLOW-SHOW-ME`: ask how to do something and have the agent demonstrate it in the interface.

## Requirements

- **PRD-ERSA-R01**: Ersa runs sessions through the user's installed vendor CLIs and never prompts for, stores, or transmits a model API key.
- **PRD-ERSA-R02**: The user can run at least ten concurrent sessions across multiple projects in one window.
- **PRD-ERSA-R03**: Every session runs in a workspace isolated from every other session's workspace.
- **PRD-ERSA-R04**: A session that is waiting on the user (permission, question, error) is distinguishable in the rail without opening it.
- **PRD-ERSA-R05**: The transcript renders agent output as formatted text with code, tool calls, diffs, and permission requests shown as distinct cards or tiles.
- **PRD-ERSA-R06**: Cost to date and context consumed are visible for the active session at all times.
- **PRD-ERSA-R07**: All sessions survive an application restart and resume with their full transcript.
- **PRD-ERSA-R08**: A session can run on a remote host over SSH with the same features as a local session.
- **PRD-ERSA-R09**: Backends are pluggable; adding a backend does not change the interface.
- **PRD-ERSA-R10**: A pack can be fetched from a pack source, applied to a project, and updated when a new version is published.
- **PRD-ERSA-R11**: The user can choose a mode per session that changes the agent's base instructions.
- **PRD-ERSA-R12**: Light and dark themes are both complete and switchable at runtime.
- **PRD-ERSA-R13**: The agent can, with the user's consent, drive the interface to demonstrate a task to the user.
- **PRD-ERSA-R14**: Version control isolation is pluggable; git worktrees are the first implementation.

## Out of scope

- Running agents against model APIs directly.
- Being a code editor. The workbench shows files and diffs; it does not replace an editor.
- Hosting or running the pack source as a service. A pack source is a directory, share, or HTTP endpoint the user already has.
- Mobile or web clients.
- macOS and Linux for the first dogfood milestone (the core must not preclude them).

## Open questions

- Which Copilot CLI protocol version to target, and whether Codex and Gemini CLI follow.
- How much of each vendor's native permission model to surface versus normalise.
- Whether modes are per session, per workspace, or per project by default.

## Changes

- 2026-10-31: created.
