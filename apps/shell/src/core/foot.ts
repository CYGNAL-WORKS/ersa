// What the rail's foot shows for Core's connection (LAY-SHELL, The rail: The
// foot). Silent while all is well; a problem line while Ersa can't reach the
// user's sessions; the label of a labeled endpoint in a debug build.
// Kept free of React so a test can check every state says the right thing.

import type { CoreView } from "./connection";

export type Foot =
  /** First connecting, or connected to the user's own Core: the foot says nothing. */
  | { kind: "silent" }
  /** The connection dropped and Ersa is attaching again, or Retry was pressed and the try is under way. */
  | { kind: "reconnecting"; reason?: string }
  /** Ersa has stopped trying; Retry attaches again. */
  | { kind: "unreachable"; reason?: string }
  /** A debug build attached to a labeled endpoint, such as a replay. */
  | { kind: "attached"; label: string };

/** An empty reason is no reason: the foot shows a line beneath only when there is text to put there. */
const reasonOf = (text: string): { reason?: string } => (text.trim() === "" ? {} : { reason: text });

/** `retried`: Retry has been pressed since the window opened, so a connect under way is a try the user asked for. */
export function footOf(view: CoreView, retried = false): Foot {
  switch (view.phase) {
    case "connecting":
      // The first connect is quiet: the tree says it is loading. A Retry's try says so, as a reattach does; its
      // reason is not known yet.
      return retried ? { kind: "reconnecting" } : { kind: "silent" };
    case "connected":
      return view.label === undefined ? { kind: "silent" } : { kind: "attached", label: view.label };
    case "lost":
      return { kind: "reconnecting", ...reasonOf(view.reason) };
    case "failed":
      return { kind: "unreachable", ...reasonOf(view.message) };
  }
}
