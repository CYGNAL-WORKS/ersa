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
    // The shelf (docs/design/RAIL.md, The shelf; FLOW-APPROVE-PERMISSION): a session's request, pulled out of its tile.
    shelf: {
      // Heard, not shown: the shelf's name, and its chevron's.
      nameLabel: "Request from {title}",
      permissionHeading: "Permission request",
      questionHeading: "Question",
      // The shown request's place among the session's requests, at the heading line's trailing end.
      placeLabel: "{n} of {m}",
      pathLabel: "Path:",
      loadingProgress: "Loading…",
      readError: "Ersa can't read this session's requests.",
      retryButton: "Retry",
      unknownNote: "This session is waiting on a request Ersa can't show here.",
      // Names the shelf's contents when they fail to draw: "This request could not be shown:".
      boundaryName: "This request",
      denyButton: "Deny",
      allowButton: "Allow",
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
