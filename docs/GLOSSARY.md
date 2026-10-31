# Glossary

One term per concept. Use these words in briefs, identifiers, and UI copy. Add a term here before using a new concept anywhere else.

## Product

- **Ersa**: the application. Always capitalised as a proper noun in prose, lowercase `ersa` as an identifier.
- **Backend**: a vendor agent product that Ersa drives, such as Claude Code or GitHub Copilot. The user installs and authenticates it themselves.
- **Adapter**: the Ersa code that drives one backend and translates its native protocol into Ersa events. One adapter per backend.
- **Mode**: a named base-instruction profile applied to a session, such as Code or Content, that steers which tools and approaches the agent reaches for first.
- **Pack**: a versioned bundle of agents, skills, MCP servers, and prompts that can be shared and applied to sessions. Packs are fetched from a pack source and cached locally.
- **Pack source**: a location packs are fetched from: a directory, a shared drive, or an HTTP endpoint.

## Work

- **Project**: a repository or root folder the user has added to Ersa.
- **Workspace**: an isolated checkout belonging to a project where one or more sessions do their work. A git worktree is one kind of workspace; a Perforce workspace is another.
- **Session**: one conversation with one agent, in one workspace, on one host. A session has a transcript and a lifecycle.
- **Host**: a machine where sessions can run. The local machine is a host; an SSH-reachable machine or a container is another.
- **Transport**: the mechanism the core uses to start and talk to processes on a host: local spawn, SSH, or container exec.
- **Event**: one typed message in Ersa's agent event protocol. Adapters emit events; the UI renders them; the event log stores them.
- **Event log**: the persisted, ordered sequence of events for a session. The transcript is rendered from it.
- **Permission request**: an event in which the agent asks the user to approve an action before it runs.

## Interface

- **Shell**: the window's fixed structure: rail, transcript, composer, workbench.
- **Rail**: the left column listing projects, their workspaces, and the sessions inside them.
- **Transcript**: the main area rendering a session's event log as messages, cards, and tiles.
- **Composer**: the input area at the bottom of the transcript where the user writes to the agent.
- **Workbench**: the right-hand area holding panes.
- **Pane**: a purpose-built view in the workbench, such as Files, Diff, Terminal, or Browser.
- **Card**: a rich block rendered inline in the transcript for a single event, such as a tool call or a diff.
- **Tile**: a compact status block, such as a permission tile or a cost tile, shown in the transcript or the rail.
- **Show me**: a guided-help mode in which the agent drives the interface to demonstrate something to the user, with the user watching.

## Process

- **Brief**: a source-of-truth document describing behaviour from the user's perspective. Types: product, flow, layout, component, protocol, architecture.
- **Requirement**: one testable statement inside a brief, carrying a stable ID such as `LAY-SHELL-R03`.
- **ADR**: an architecture decision record. One decision per file, superseded rather than edited.
- **Manifest**: the generated index of briefs, their status, and requirement coverage.
- **Ticket**: an ephemeral unit of implementation work sized to one fresh agent context, derived from a brief.
