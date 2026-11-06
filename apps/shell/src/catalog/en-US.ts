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
      noProjectsHint: "Start with New session, where you can add one.",
      projectsError: "Ersa can't read your projects.",
      sessionsError: "Ersa can't read your sessions.",
      // Names the tree when it fails to draw: "The projects and sessions could not be shown:".
      boundaryName: "The projects and sessions",
    },
    // A project heading (docs/design/RAIL.md, A project heading) and its More menu.
    heading: {
      newSessionLabel: "New session in {project}",
      moreLabel: "More for {project}",
      notTrustedStatus: "Not trusted",
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
  },
  transcript: {},
  composer: {},
  newSession: {},
  sessionControls: {
    // The context ring's name, in the session controls and on a tile.
    contextRing: {
      noneLabel: "Context: none used yet",
      usedLabel: "Context: {percent} percent used",
    },
  },
  quit: {},
  trust: {},
  errors: {},
} as const satisfies Section;
