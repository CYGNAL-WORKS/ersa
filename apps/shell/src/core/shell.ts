// Closing and quitting (FLOW-CLOSE-AND-QUIT). Closing the window only hides
// it; Quit, from the tray icon's menu, reaches the window as a request, and
// the window asks the user to confirm when Quit would stop busy sessions.

import type { Session } from "../protocol/core-api.gen";
import type { CoreClient } from "./client";

/** The Shell's controls the window uses, beside the bridge to Core. */
export interface ShellControl {
  /** Calls `handler` each time the user chooses Quit from the tray icon; returns a way to stop. */
  onQuitRequested(handler: () => void): () => void;
  /** Shows the window, hidden to the tray or not. */
  show(): Promise<void>;
  /** Shuts Core down and exits once it has stopped. Confirmation, if any, comes first. */
  quit(): Promise<void>;
}

const BUSY = new Set<string>(["starting", "running", "waiting_user"]);

/** The sessions a quit would interrupt: working, starting, or waiting on the user (FLOW-CLOSE-AND-QUIT-R04). */
export function busySessions(sessions: readonly Session[]): number {
  return sessions.filter((s) => BUSY.has(s.tile.state)).length;
}

/**
 * Answers a Quit from the tray. With no busy session it quits at once and
 * returns 0; otherwise it shows the window and returns how many would stop,
 * for the window to ask about, and nothing is sent until the user confirms.
 * Sessions are read afresh; when Core cannot be asked, `known` (the rail's
 * last list) stands in.
 */
export async function answerQuit(client: CoreClient, shell: ShellControl, known: readonly Session[] = []): Promise<number> {
  const sessions = await client.call("session.list").catch(() => known);
  const busy = busySessions(sessions);
  if (busy === 0) await shell.quit();
  else await shell.show();
  return busy;
}
