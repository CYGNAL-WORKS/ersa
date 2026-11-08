// Closing and quitting (FLOW-CLOSE-AND-QUIT). Closing the window only hides
// it; Quit, from the tray icon's menu, reaches the window as a request, and
// the window asks the user to confirm when Quit would stop busy sessions.

import type { Session } from "../protocol/core-api.gen";
import type { CoreClient } from "./client";

/**
 * A drag from the file manager over the window (FLOW-SEND-MESSAGE-R08): the pointer moving over it, in CSS pixels
 * from the window's top left; the paths dropped there; or the drag leaving or canceled.
 */
export type DragDrop =
  | { type: "over"; x: number; y: number }
  | { type: "drop"; x: number; y: number; paths: string[] }
  | { type: "leave" };

/** The Shell's controls the window uses, beside the bridge to Core. */
export interface ShellControl {
  /** Calls `handler` each time the user chooses Quit from the tray icon; returns a way to stop. */
  onQuitRequested(handler: () => void): () => void;
  /** Tells the Shell the window heard a Quit and will answer; one that does not hear is quit without it. */
  quitHeard(): Promise<void>;
  /** Shows the window; whether it was hidden to the tray before. */
  show(): Promise<boolean>;
  /** Hides the window to the tray again, as it was before a quit question showed it. */
  hide(): Promise<void>;
  /** Shuts Core down and exits once it has stopped. Confirmation, if any, comes first. */
  quit(): Promise<void>;
  /** Opens the system's folder dialog on this computer; the folder the user picked, or null when they canceled. */
  pickFolder(): Promise<string | null>;
  /**
   * Opens the system's dialog for files, or for images alone, on this computer: the paths the user picked, none when
   * they canceled (FLOW-SEND-MESSAGE-R07, R13).
   */
  pickFiles(images: boolean): Promise<string[]>;
  /** Hears files, images and folders dragged from the file manager over the window; returns a function that stops. */
  onDragDrop(handler: (event: DragDrop) => void): () => void;
}

const BUSY = new Set<string>(["starting", "running", "waiting_user"]);

/** How long to wait for Core's session list before counting the rail's own. */
export const QUIT_LIST_TIMEOUT_MS = 2_000;

/** Whether a session is working, starting, or waiting on the user: Quit would interrupt it, and Delete waits for it. */
export const isBusy = (session: Session): boolean => BUSY.has(session.tile.state);

/** The sessions a quit would interrupt (FLOW-CLOSE-AND-QUIT-R04). */
export function busySessions(sessions: readonly Session[]): number {
  return sessions.filter(isBusy).length;
}

/**
 * What the window asks about after a Quit: how many sessions would stop (`undefined` when they could not be
 * counted), and whether the window was hidden.
 */
export interface QuitAsk {
  busy: number | undefined;
  wasHidden: boolean;
}

/**
 * Answers a Quit from the tray. With no busy session it quits at once and
 * returns `undefined`; otherwise it shows the window and returns what to ask,
 * and nothing is sent until the user confirms.
 * Sessions are read afresh; when Core cannot be asked, `known` (the rail's
 * last list) stands in, and with neither the window asks anyway: a Core
 * left running by an earlier window may have busy sessions.
 */
export async function answerQuit(
  client: CoreClient,
  shell: ShellControl,
  known?: readonly Session[],
): Promise<QuitAsk | undefined> {
  await shell.quitHeard();
  // A Core slow to answer must not hold Quit up: the rail's last list stands in.
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error("timed out")), QUIT_LIST_TIMEOUT_MS);
  });
  const sessions = await Promise.race([client.call("session.list"), timeout])
    .catch(() => known)
    .finally(() => clearTimeout(timer));
  const busy = sessions && busySessions(sessions);
  if (busy === 0) {
    await shell.quit();
    return undefined;
  }
  return { busy, wasHidden: await shell.show() };
}

/** Cancelling a quit: the window goes back to the tray if that is where it was, and nothing else changes. */
export async function cancelQuit(shell: ShellControl, ask: QuitAsk): Promise<void> {
  if (ask.wasHidden) await shell.hide();
}
