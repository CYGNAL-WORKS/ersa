---
id: PRD-ERSA
type: product
title: Ersa
status: ready
verified-by: review
references: [FLOW-NEW-SESSION, FLOW-APPROVE-PERMISSION, FLOW-RESUME-SESSION, FLOW-REMOTE-SESSION, FLOW-APPLY-PACK, FLOW-SHOW-ME]
adrs: [0002, 0004, 0006, 0007, 0009, 0010, 0011, 0012]
---

# Ersa

## Problem

Running several coding agents at once is now normal, and the vendors' own desktop apps handle the basics well: parallel sessions, isolated checkouts, remote machines, a side pane. Three things they do not do, and will not, because they are not in the vendor's interest:

1. **Let you extend the harness.** The tools the agent can call about its own environment (other sessions, the interface, the team's conventions) are whatever the vendor ships. You cannot add your own.
2. **Let you change vendors.** Each app is one model family. If the right model next year is a different vendor's, or a local one, you start over: new app, new habits, new session history.
3. **Work outside git.** Isolation per session means a git worktree. Game development and much of the rest of the industry runs on Perforce or other version control, where the vendor apps offer nothing.

Alongside those, the everyday problem that started this: with ten sessions open you need one glance to know which ones are waiting on you, what each has consumed, and what it is doing, without opening each one.

## Who it is for

- A developer running two to ten agent sessions in parallel, across repositories and machines, who wants one window and one glance.
- A game developer whose projects live in Perforce, who gets none of the isolation or workflow the vendor apps give git users.
- A developer or team who wants their own tools available to every agent session, and who wants the option of switching or mixing vendors, including local inference, without changing tools.
- A team lead who wants to share a curated set of agents, skills, and tools with the team without a git tutorial.

## Solution

Ersa is a desktop application that hosts many agent sessions in one window. Each session drives a vendor's own CLI (Claude Code first, GitHub Copilot second, others and local inference later) through that CLI's structured-event interface, using the login the user already has. Each session runs in an isolated workspace on a local or remote host; isolation is provided by a pluggable version-control layer, git worktrees first and Perforce next.

The window has four regions. The rail lists projects, their workspaces, and their sessions; each session is a rich tile showing title, backend, activity state, a context ring, and cost as the backend reports it, and a tile pulls out a shelf when the session needs the user (a permission, a question) so it can be answered without leaving the current session. The transcript renders the active session as a document with cards for tool calls and diffs. The composer is where the user writes. The workbench holds purpose-built panes: files, diff, a full hosted terminal in the workspace, browser.

Ersa exposes its own tool server to every session, so agents can message other sessions, read session state, and, with consent, show the user how to do something in the interface. Users can add their own tools to that server. Packs are a neutral description of agents, skills, tool servers, and prompts, rendered into each backend's native format and fetched from a simple versioned source. Modes are named presets (base instructions, allowed tools, tool servers) chosen per session.

## Principles

- **The user's existing tools, the user's existing auth.** Ersa drives CLIs the user already has. It never asks for or stores a model API key.
- **Structured events, never console scraping.** Every backend is driven through a machine-readable protocol. If a backend has none, it is not supported.
- **Many sessions, one glance.** Anything that needs the user is visible and actionable from the rail without opening the session.
- **Isolation by default, whatever the VCS.** A session cannot accidentally work in another session's checkout, in git or otherwise.
- **Local and remote feel the same.** A remote session has the same tiles, panes, and shelf as a local one; only latency differs.
- **The harness is yours.** Ersa's own tools are extensible by the user, and nothing about a backend is hard-wired into the interface.
- **Beautiful and calm.** The interface is an instrument, not a chat toy. See `docs/design/DESIGN.md`.
- **Dogfood first.** The first usable version is the one its author switches to daily.

## Flows

- `FLOW-NEW-SESSION`: add a project, create a workspace, start a session with a chosen backend and mode.
- `FLOW-APPROVE-PERMISSION`: a session needs the user; answer it from the rail shelf without switching sessions.
- `FLOW-RESUME-SESSION`: close Ersa, reopen it, and continue every session where it left off.
- `FLOW-REMOTE-SESSION`: add a remote host, create a workspace there, run a session on it.
- `FLOW-APPLY-PACK`: subscribe to a pack source, apply a pack to a project, receive an update.
- `FLOW-SHOW-ME`: ask how to do something and have the agent demonstrate it in the interface.

## Requirements

Verified by review at this level; testable requirements live in the flow and protocol briefs.

- **PRD-ERSA-R01**: Ersa runs sessions through the user's installed vendor CLIs and never prompts for, stores, or transmits a model API key.
- **PRD-ERSA-R02**: Every backend is driven through a structured-event protocol; Ersa does not parse terminal output.
- **PRD-ERSA-R03**: The user can run at least ten concurrent sessions across multiple projects and hosts in one window.
- **PRD-ERSA-R04**: Every session runs in a workspace isolated from every other session's workspace.
- **PRD-ERSA-R05**: Workspace isolation is pluggable by version control system; git worktrees are the first implementation and Perforce the second.
- **PRD-ERSA-R06**: A session that needs the user (permission, question, error) is visible in the rail and can be answered from the rail without making it the active session.
- **PRD-ERSA-R07**: Each session's rail tile shows its title, backend, activity state, context consumed, and cost as reported by the backend.
- **PRD-ERSA-R08**: The transcript renders agent output as formatted text, with tool calls, diffs, and permission requests as distinct cards.
- **PRD-ERSA-R09**: All sessions survive an application restart and resume with their full transcript.
- **PRD-ERSA-R10**: A session can run on a remote host with the same features as a local session.
- **PRD-ERSA-R11**: Backends are pluggable; adding a backend, including a local-inference backend, does not change the interface.
- **PRD-ERSA-R12**: Ersa exposes a tool server to every session, and the user can add their own tools to it.
- **PRD-ERSA-R13**: A pack is a backend-neutral description rendered into each backend's native format; it can be fetched from a pack source, applied to a project, and updated.
- **PRD-ERSA-R14**: The user can choose a mode per session; a mode is a named preset of base instructions, allowed tools, and tool servers.
- **PRD-ERSA-R15**: Light and dark themes are both complete and switchable at runtime.
- **PRD-ERSA-R16**: With the user's visible consent, the agent can drive the interface to demonstrate a task, and can never operate permission controls.

## Out of scope

- Calling model APIs directly.
- Driving any backend through a pseudo-terminal or by parsing console output.
- Being a code editor. The workbench shows files and diffs; it does not replace an editor.
- Hosting the pack source as a service. A pack source is a directory, share, or HTTP endpoint the user already has.
- A content-oriented mode or persona until a named user exists for it.
- Mobile or web clients.
- macOS and Linux for the first dogfood milestone (the core must not preclude them).

## Open questions

- Which local-inference harness to target first as proof that backends are pluggable beyond the two vendors.
- How much of each vendor's native permission vocabulary to surface versus normalise.
- Whether modes are per session, per workspace, or per project by default.
- Perforce isolation model: per-session workspace with its own client spec, or shared workspace with changelists.

## Changes

- 2026-10-31: rewritten after adversarial review: problem statement centred on the three real differentiators (extensible harness, vendor-agnostic, non-git VCS); shelf-based attention from the rail; rail tile contents specified; structured-events principle; packs as neutral definition; content mode deferred.
- 2026-10-31: created.
