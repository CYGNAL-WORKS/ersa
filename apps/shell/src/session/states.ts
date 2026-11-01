// How the window names a session's lifecycle state (PROTO-AGENT-EVENTS,
// session.state), in the transcript's header and on its tile.

import type { SessionState } from "../protocol/agent-events.gen";

export function stateText(state: SessionState["state"] | undefined): string {
  switch (state) {
    case undefined:
      return "";
    case "starting":
      return "Starting";
    case "running":
      return "Working";
    case "waiting_user":
      return "Needs you";
    case "idle":
      return "Idle";
    case "disconnected":
      return "Disconnected";
    case "ended":
      return "Ended";
    case "error":
      return "Error";
  }
}
