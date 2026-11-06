// The string catalog's US English source (docs/design/CONTENT.md, "The string
// catalog"). Every string the Interface shows lives here, keyed by area and
// purpose, and a key's suffix names its kind (`Button`, `Error`, `Progress`,
// `Status`, `Placeholder`, `Empty`, `Confirmation`), which the catalog test
// checks. Another locale is a sibling file with the same keys.
//
// After a change, run `node scripts/gen-catalog.mjs` to rewrite the text
// export, en-US.gen.md, which is what review and Vale read.
//
// Plain data with no runtime imports, so that script can load it directly.

import type { Section } from "./lookup";

export const enUS = {
  rail: {
    // The rail's tree (docs/design/RAIL.md, The rail): its name and its notes.
    tree: {
      // The tree's name for assistive technology.
      label: "Projects and sessions",
      loadingProgress: "Loading…",
      noProjectsEmpty: "No projects yet.",
      // A project is added from the new-session screen (LAY-SHELL, The rail).
      noProjectsHint: "Choose a folder in New session to start.",
      projectsError: "Ersa can't read your projects.",
      sessionsError: "Ersa can't read your sessions.",
      // Names the tree when it fails to draw: "The projects and sessions could not be shown:".
      boundaryName: "The projects and sessions",
    },
    // A project heading (docs/design/RAIL.md, A project heading) and its More menu.
    heading: {
      newSessionLabel: "New session in {project}",
      moreLabel: "More for {project}",
      // In the More menu. Trusting asks first, so it ends in an ellipsis; withdrawing trust does not ask.
      trustButton: "Trust…",
      distrustButton: "Don't trust",
      savingProgress: "Saving…",
      trustError: "Ersa can't change this project's trust.",
      workspacesError: "Ersa can't read this project's workspaces.",
      // Beneath a project whose trust was withdrawn while sessions that started trusted still run.
      keepTrust: "Running sessions keep their current settings until they are stopped.",
    },
    foot: {
      attachedLabel: "Attached to {label}",
      reconnectingError: "Ersa can't reach your sessions. It's trying again.",
      unreachableError: "Ersa can't reach your sessions.",
      retryButton: "Retry",
      // Heard, not shown: assistive technology is told when Ersa reaches the sessions again after a problem.
      reconnectedStatus: "Reconnected",
      // The reason beneath the foot's line when the handshake can't be read.
      handshakeError: "Ersa can't read the connection's handshake. This window may need updating.",
      // Names the foot when it fails to draw: "The connection status could not be shown:".
      boundaryName: "The connection status",
    },
    // A session's tile (docs/design/RAIL.md, A tile): line two's phrase, and the names of its marks.
    tile: {
      untitledLabel: "Untitled session",
      // Names a tile when it fails to draw: "This session could not be shown:".
      boundaryName: "This session",
      startingStatus: "Starting",
      // Heard, not shown: the working mark's name, since line two has no phrase for it.
      workingStatus: "Working",
      disconnectedStatus: "Disconnected",
      errorStatus: "Error",
      endedStatus: "Ended",
      unknownStateStatus: "Unknown state",
      // What a session that needs the user wants, in the tone of its chevron.
      permissionWant: "Needs permission",
      questionWant: "Has a question",
      needsYouWant: "Needs you",
      needsYouCountWant: "Needs you · {count}",
    },
    // The shelf (docs/design/RAIL.md, The shelf; FLOW-APPROVE-PERMISSION): a session's request, pulled out of its tile.
    shelf: {
      // Heard, not shown: the shelf's name, and its chevron's.
      nameLabel: "Request from {title}",
      permissionHeading: "Permission request",
      questionHeading: "Question",
      // The shown request's place among the session's requests, at the heading line's trailing end.
      placeLabel: "{n} of {m}",
      // Heard, not shown, as the shelf moves to the next request: "Permission request, 2 of 3: Run the tests".
      nextLabel: "{kind}, {n} of {m}: {asked}",
      // Heard as the shelf moves to a request it can't show: "2 of 3: This session is waiting on…".
      nextUnknownLabel: "{n} of {m}: {note}",
      pathLabel: "Path:",
      loadingProgress: "Loading…",
      readError: "Ersa can't read this session's requests.",
      retryButton: "Retry",
      unknownNote: "This session is waiting on a request Ersa can't show here.",
      // Names the shelf's contents when they fail to draw: "This request could not be shown:".
      boundaryName: "This request",
      // A permission request's answers: the transcript's permission card offers the same ones (R27).
      denyButton: "Deny",
      allowButton: "Allow",
      // Followed by Core's reason, beneath.
      answerRefusedError: "Your answer wasn't accepted.",
      reconnectingNote: "Reconnecting to Core…",
    },
  },
  transcript: {},
  composer: {},
  newSession: {},
  sessionControls: {
    // The host chip's name for the host Ersa runs on.
    localHostLabel: "Local",
    // A host in the host chip's list that has no project to start a session in.
    noProjectsStatus: "No projects",
    // A project in the project chip's list (docs/design/COMPOSER.md, The project list): its kind, a git
    // repository or any other folder, and the action that ends the list.
    project: {
      withGitKindLabel: "Git",
      withoutGitKindLabel: "Folder",
      addProjectButton: "Add project…",
      // The keys that choose Add project from anywhere on the new-session screen.
      addProjectShortcutLabel: "Ctrl+O",
      // The project chip while the local host has no project: choosing it, or sending, opens the folder dialog.
      chooseFolderButton: "Choose a folder",
    },
    // The context ring's name, in the session controls and on a tile.
    contextRing: {
      noneLabel: "Context: none used yet",
      usedLabel: "Context: {percent} percent used",
    },
  },
  quit: {},
  trust: {
    // A project the user has not trusted, on its heading in the rail and in the project chip's list.
    notTrustedStatus: "Not trusted",
    // The trust question (ADR 0018), asked when a folder becomes a project and when the user trusts one later. It
    // says what trust lets run, that it covers every workspace of the project, and that it lasts until withdrawn
    // (FLOW-NEW-SESSION-R16, retired; #226 builds its replacement).
    question: {
      title: "Trust {name}?",
      runsDetail:
        "A trusted project’s own configuration runs in its sessions: hooks and MCP servers that start programs on this computer, and settings that change what the agent may do without asking.",
      coversDetail:
        "Trust covers every workspace of the project and lasts until you withdraw it. Claude Code also records it for the whole repository the project is in, and applies it outside Ersa.",
      untrustedDetail: "Untrusted, its sessions use only your own settings.",
      addTrustedButton: "Add, trusted",
      addUntrustedButton: "Add, not trusted",
      trustButton: "Trust",
      cancelButton: "Cancel",
    },
  },
  errors: {},
} as const satisfies Section;
