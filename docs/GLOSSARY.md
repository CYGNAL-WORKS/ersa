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
- **Trusted project**: a project whose own configuration (settings, hooks, MCP servers, permission rules) the user has agreed may run in its sessions. Asked when the project is added; an untrusted project's sessions run with the user's own configuration only.
- **Workspace**: a checkout belonging to a project where one or more sessions do their work: an isolated workspace (a git worktree is one kind; a Perforce workspace is another) or the project's root workspace.
- **Root workspace**: a project's own folder as a workspace. Every project has one. It is not isolated: its sessions share its files with each other and with the user. A session runs there when the user turns the worktree off.
- **Effort**: how much reasoning a backend spends per turn, in the backend's own levels. Chosen when a session starts.
- **Permission mode**: a backend's rule for which of the agent's actions ask the user first, in the backend's own names. Chosen when a session starts.
- **Session**: one conversation with one agent, in one workspace, on one host. An ordinary backend session: its history is the backend's session record, and Ersa stores only what finds it again (host, project, workspace, backend, and the backend's session id) and its mode. Its title is the backend's.
- **Session record**: the backend's own record of a session, which the backend resumes from and its own tools list. The history of a session, whether its turns were taken in Ersa or elsewhere. Retention is the backend's.
- **Live stream**: what a running backend sends Ersa as it happens. Carries details the session record omits (text as it arrives, permission prompts, turn ends, cost, rate limits), which are shown and not stored.
- **Host**: a machine where sessions can run. The local machine is a host; an SSH-reachable machine is another. A remote host runs its own Core.
- **Transport**: the mechanism Core uses to start and talk to processes on a host: local spawn or SSH.
- **Raw log**: the verbatim, ordered lines exchanged with a live session's backend since Core opened it, each tagged with its direction, held in memory for the developer pane and never stored. Not a source of truth.
- **Projection**: the versioned derivation of Ersa events from a session record and the live stream, merged on the backend's message ids. Made each time a session is opened, never stored. Transcript and rail state are projections.
- **View**: one projection of a session that Core holds for the session's subscribers, with its own identity and sequence numbers. When Core rebuilds a session's projection it is a new view.
- **Event**: one typed message in the agent event protocol, produced by projection and streamed to the Interface.
- **Turn**: one message from the user and everything the agent does in answer to it, up to the user's next message. *Not: exchange, round.*
- **Permission request**: the agent asking the user to approve an action before it runs.
- **Question**: the agent asking the user to choose or answer something that is not a permission.
- **Replay server**: a stand-in Core that serves recorded fixtures over the Interface protocols, used for interface development and tests.

## System

- **Shell**: the native window process that hosts the Interface and launches or attaches to Core.
- **Tray icon**: the Shell's icon in the system tray. Closing the window hides it there; the icon reopens the window and carries Quit.
- **Quit**: stopping Ersa entirely: the Shell asks Core to stop every session and exit, then exits itself. Closing the window is not quitting.
- **Interface**: the TypeScript application in the Shell's webview. *Not: frontend.*
- **Core**: the Rust process that owns sessions, adapters, transports, workspaces, packs, modes, the harness tools server, and persistence. *Not: daemon.*
- **Core API**: the request/response protocol between Interface and Core.
- **App log**: Core's log of its own operational records (process started, exited, failed to start, Core restarted). Not part of any session record; Core reads it back at startup for each session's first and last facts.
- **Harness tools server**: the MCP tools Core exposes to every session's backend: cross-session messaging, session metadata, user-defined tools, and interface-driving tools. Never permission prompting.

## Interface

- **Rail**: the left column listing projects, their workspaces, and the sessions inside them. *Not: sidebar.*
- **Tile**: a session's entry in the rail: title, backend icon, activity state, context ring, and cost as reported by the backend.
- **Shelf**: the panel that pulls out from a tile when the session needs the user (a permission request or a question), answerable without changing the active session. *Not: drawer, flyout.*
- **Context ring**: the circular gauge on a tile showing context consumed.
- **Transcript**: the main area rendering the active session's projection as a document with cards. *Not: chat history, chat log.*
- **Composer**: the input area beneath the transcript where the user writes to the agent. *Not: chat box, input box, prompt box.*
- **Session controls**: the row of chips beneath the composer naming where and how a session runs: host, project, workspace, backend, model, effort and permission mode, with the context ring. Set before the first message; shown during the session.
- **New-session screen**: what the transcript area shows while no session is active: the composer and the session controls, ready for the first message, which starts the session.
- **Workbench**: the right-hand area holding panes.
- **Pane**: a purpose-built area in the workbench: Files, Diff, Terminal, Browser, Developer.
- **Terminal pane**: a full hosted terminal (a real shell in a pseudo-terminal) opened in the session's workspace, for the user. Backends are never driven through it; the agent's own commands appear as cards in the transcript.
- **Card**: a rich block rendered inline in the transcript for a single event, such as a tool call or a diff.
- **Theme**: light or dark. Follows the system unless the user chooses one. Every palette has both.
- **Palette**: a named, complete set of color tokens for both themes, such as Midnight, the default. The user chooses one; components name color roles, never a palette's values.
- **String catalog**: the one place every string the Interface shows is kept, keyed by area and purpose, with named values and plural forms; components refer to keys. US English is its source locale. *Not: string table, resource file.*
- **Show me**: a guided-help mode in which the agent, with the user's visible consent, drives the interface to demonstrate something. It can never operate permission controls.

## Process

- **Brief**: a source-of-truth document describing behavior from the user's perspective. Types: product, flow, layout, component, protocol, architecture.
- **Requirement**: one testable statement inside a flow or protocol brief, carrying a stable ID such as `FLOW-NEW-SESSION-R03`.
- **ADR**: an architecture decision record. One decision per file, superseded rather than edited.
- **Inbox**: the committed file where implementing sessions record findings that need a planning pass.
- **Manifest**: the generated index of briefs, their status, requirement coverage, and problems.
- **Ticket**: an ephemeral unit of implementation work sized to one fresh agent context, derived from a brief.
- **Planning session**: an agent session that may edit briefs and ADRs. Declares itself with the planning role.
- **Implementing session**: an agent session that builds tickets, may not edit briefs or ADRs, and writes findings to the inbox.
