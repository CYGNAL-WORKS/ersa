# Glossary

One term per concept. Use these words in briefs, identifiers, and UI copy. Add a term here before using a new concept anywhere else. A term that ends in a *Not:* list names the words it replaces; Vale's `Ersa.Terms` rule flags them everywhere it checks except this file, so list only words that never mean something else in Ersa's docs, including another product's concepts.

## Product

- **Ersa**: the application. Capitalized as a proper noun in prose, lowercase `ersa` as an identifier.
- **Backend**: a vendor agent product that Ersa drives, such as Claude Code or GitHub Copilot, or a local-inference harness. The user installs and authenticates it themselves. *Not: the AI.*
- **Adapter**: the Ersa code that drives one backend through its structured-event protocol and provides the projection from that backend's live stream and session record to Ersa events. One adapter per backend.
- **Mode**: a named preset applied to a session at start: base instructions, allowed tools, and tool servers.
- **Pack**: a backend-neutral, versioned description of agents, skills, tool servers, and prompts, rendered into each backend's native format when applied to a project.
- **Pack source**: a location packs are fetched from: a directory, a shared drive, or an HTTP endpoint.

## Work

- **Project**: a repository or root folder the user has added to Ersa, on a particular host.
- **Trusted project**: a project whose own configuration (settings, hooks, MCP servers, permission rules) the user has agreed may run in its sessions. A project is trusted when the user adds it, after the trust question; the user can withdraw trust later, and an untrusted project's sessions run with the user's own configuration only.
- **Workspace**: a checkout belonging to a project where one or more sessions do their work: an isolated workspace (a git worktree is one kind; a Perforce workspace is another) or the project's root workspace.
- **Root workspace**: a project's own folder as a workspace. Every project has one. It is not isolated: its sessions share its files with each other and with the user. A session runs there when the user chooses it over a new workspace. The session controls call it the project folder.
- **Base branch**: the branch a new isolated workspace is made from. Starts at the repository's default branch: the one `origin/HEAD` names, else main or master, else what the project's folder has checked out.
- **Scratch folder**: the folder inside a workspace where Ersa puts copies of attachments from outside it, ignored by version control.
- **Effort**: how much reasoning a backend spends per turn, in the backend's own levels. Chosen when a session starts.
- **Permission mode**: a backend's rule for which of the agent's actions ask the user first, in the backend's own names. Chosen when a session starts.
- **Session**: one conversation with one agent, in one workspace, on one host. An ordinary backend session: its history is the backend's session record, and Ersa stores only what finds it again (host, project, workspace, backend, and the backend's session id), its mode, the user's arrangement of it, and the values it last asked the backend for (ADR 0033). Its title is the backend's.
- **Fork**: a new session that starts from a copy of another session's conversation up to its last finished reply, in a workspace of its own when the original is in an isolated one and in the same root workspace otherwise, and continues on its own. The session it was made from is unchanged.
- **Archived session**: a session the user has put away: stopped, hidden from the rail unless the user shows archived sessions, and opened read-only until the user resumes or unarchives it. Its record and workspace stay.
- **Dormant session**: a session whose backend has no adapter in this build. Ersa shows it nowhere and cannot run it, keeps what it stores about it, and shows it again once the adapter is back (ADR 0031). *Not: archived session, which the user put away.*
- **Session record**: the backend's own record of a session, which the backend resumes from and its own tools list. The history of a session, whether its turns were taken in Ersa or elsewhere. Retention is the backend's, except that deleting a session in Ersa deletes its record.
- **Live stream**: what a running backend sends Ersa as it happens. Carries details the session record omits (text as it arrives, permission prompts, turn ends, cost, rate limits), which are shown and not stored.
- **Host**: a machine where sessions can run. The local machine is a host; an SSH-reachable machine is another. A remote host runs its own Core.
- **Transport**: the mechanism Core uses to start and talk to processes on a host: local spawn or SSH.
- **Raw log**: the verbatim, ordered lines exchanged with a live session's backend since Core opened it, each tagged with its direction, held in memory for the developer pane and never stored. Not a source of truth.
- **Projection**: the versioned derivation of Ersa events from a session record and the live stream, merged on the backend's message ids. Made each time a session is opened, never stored. Transcript and rail state are projections.
- **View**: one projection of a session that Core holds for the session's subscribers, with its own identity and sequence numbers. When Core rebuilds a session's projection it is a new view.
- **Event**: one typed message in the agent event protocol, produced by projection and streamed to the Interface.
- **Turn**: one message to the agent and everything the agent does in answer to it, up to the next. Most turns start with the user's message; a turn can also start when a session resumes or from the system.
- **Attachment**: a file, image or folder that goes with a message. An image goes to the backend as an image; a file or folder goes as its path.
- **Permission request**: the agent asking the user to approve an action before it runs.
- **Question**: the agent asking the user to choose or answer something that is not a permission.
- **Replay capture**: what the replay server sent for one fixture, kept as a file the Interface's tests replay without running Core.
- **Replay server**: a stand-in Core that serves recorded fixtures over the Interface protocols, used for interface development and tests. It is read-only: it refuses every request that would create or change a session, project, workspace or setting.

## System

- **Shell**: the native window process that hosts the Interface and launches or attaches to Core.
- **Tray icon**: the Shell's icon in the system tray. Closing the window hides it there; the icon reopens the window and carries Quit.
- **Quit**: stopping Ersa entirely: the Shell asks Core to stop every session and exit, then exits itself. Closing the window is not quitting.
- **Interface**: the TypeScript application in the Shell's webview. *Not: frontend.*
- **Core**: the Rust process that owns sessions, adapters, transports, workspaces, packs, modes, the harness tools server, and persistence. Interface copy never names it (CONTENT.md); where a message has to refer to it, it is "Ersa's background process". *Not: daemon.*
- **Core API**: the request/response protocol between Interface and Core.
- **App log**: Core's log of its own operational records (process started, exited, failed to start, Core restarted). Not part of any session record; Core reads it back at startup for each session's first and last facts.
- **Harness tools server**: the MCP tools Core exposes to every session's backend: cross-session messaging, session metadata, user-defined tools, and interface-driving tools. Never permission prompting.

## Interface

- **Rail**: the left column listing sessions as tiles in groups, by project unless the user arranges them otherwise. *Not: sidebar.*
- **Tile**: a session's entry in the rail: backend icon, title, state, context ring, and cost as reported by the backend.
- **Project heading**: a project's row in the rail, above the tiles of its sessions. It collapses and expands them.
- **Group**: a run of tiles in the rail under one heading. The user chooses how the rail groups: by project, section or date, or not at all; pinned sessions lead in their own group whichever is chosen.
- **Section**: a group the user makes and names to arrange sessions across projects. A session is in one section or in none; a section stays while empty.
- **Pinned session**: a session the user has pinned, so it leads the rail in the Pinned group whatever the grouping. Pinning changes neither its project nor its section.
- **Arrangement**: how the user has organized their sessions: which are pinned, in which section, archived and unseen, and how the rail groups, sorts and filters them and which groups are collapsed. Set from a tile's menu and the arrange menu, and kept across restarts.
- **Tile menu**: the menu of actions on one session, opened from its tile's More button, a right-click or the context-menu key.
- **Unseen session**: a session that went idle after working while it was not the active session, or that the user marked as unread, until the user makes it active or marks it as read.
- **Foot**: the bottom of the rail. Empty unless Ersa can't reach Core.
- **Shelf**: the panel that pulls out from a tile when the session needs the user (a permission request or a question), answerable without changing the active session. The chevron on the tile is its handle. *Not: drawer, flyout.*
- **Context ring**: the circular gauge showing the share of its context window a session has used, on its tile and in the session controls.
- **Transcript**: the main area rendering the active session's projection as a document with cards. *Not: chat history, chat log.*
- **Composer**: the input area beneath the transcript where the user writes to the agent. *Not: chat box, input box, prompt box.*
- **Session controls**: the row of chips beneath the composer naming where and how a session runs: host, project, workspace, backend, model, effort and permission mode, with the context ring. Set before the first message; shown during the session, where the model, effort and permission mode can still change.
- **Sessions page**: what the transcript area shows after View all: every session in one list, active or archived, with search. Choosing one opens it.
- **New-session screen**: what the transcript area shows while no session is active: the composer and the session controls, ready for the first message, which starts the session.
- **Workbench**: the right-hand area holding panes.
- **Pane**: a purpose-built area in the workbench: Files, Diff, Terminal, Browser, Developer.
- **Terminal pane**: a full hosted terminal (a real shell in a pseudo-terminal) opened in the session's workspace, for the user. Backends are never driven through it; the agent's own commands appear as cards in the transcript.
- **Card**: a rich block rendered inline in the transcript for a single event, such as a tool call or a diff.
- **Working mark**: the animated mark at the foot of a turn, beside the elapsed time and tokens, shown while the agent works on that turn.
- **Theme**: light or dark. Follows the system unless the user chooses one. Every palette has both.
- **Palette**: a named, complete set of color tokens for both themes, such as Midnight, the default. The user chooses one; components name color roles, never a palette's values.
- **String catalog**: the one place every string the Interface shows is kept, keyed by area and purpose, with named values and plural forms; components refer to keys. US English is its source locale. *Not: string table, resource file.*
- **Show me**: a guided-help mode in which the agent, with the user's visible consent, drives the interface to demonstrate something. It can never operate permission controls.

## Process

- **Brief**: a source-of-truth document describing behavior from the user's perspective. Types: product, flow, layout, component, protocol, architecture.
- **Requirement**: one testable statement inside a flow or protocol brief, carrying a stable ID such as `FLOW-NEW-SESSION-R03`.
- **ADR**: an architecture decision record. One decision per file, superseded rather than edited.
- **Inbox**: the committed file where implementing sessions record findings that need a planning pass.
- **Merge review**: the review Docket posts on a pull request's head once CI passes, and the only review a change gets. A clean merge review lets the pull request merge (ADR 0030).
- **Docket**: the review service on the owner's machine that runs the merge review on each pull request whose CI passed, gates the merge, and answers an implementing session's question about its pull request's state (ADR 0030).
- **Assumption check**: what an implementing session runs on its plan before writing code: a fresh read-only agent tries to disprove each assumption the plan rests on.
- **Merge queue**: GitHub's queue that merges a pull request onto `main` and runs CI on that merge before `main` moves.
- **Manifest**: the generated index of briefs, their status, requirement coverage, and problems.
- **Daily build**: the Ersa the owner installs and uses every day: an unsigned installer built locally from `main`, running on its own endpoint label and data folder so development builds and live checks never touch it (ADR 0035). *Not: release, production build.*
- **Reviewer agent**: an agent the merge review runs on one diff for one concern (Spec, Code, Surface or Design) that reports only findings it confirmed.
- **Ticket**: an ephemeral unit of implementation work sized to one fresh agent context, derived from a brief.
- **Planning session**: an agent session that may edit briefs and ADRs. Declares itself with the planning role.
- **Implementing session**: an agent session that builds tickets, may not edit briefs or ADRs, and writes findings to the inbox.
