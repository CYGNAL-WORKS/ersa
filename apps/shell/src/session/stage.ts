// What the stage shows and how it changes: the active session's transcript, or the new-session screen while the user
// has chosen New session or has no session (LAY-SHELL, FLOW-NEW-SESSION). Kept free of React so the tests drive each
// transition; the window holds it with `useReducer`.

import type { Sending } from "./follow";
import type { FirstSendFailure } from "./start";

export interface StageState {
  /** The session the stage shows when the new-session screen is not up. */
  activeId?: string;
  /**
   * The new-session screen, while the user has chosen New session for a project (or for none, from the screen
   * shown with no session). `key` is new on each choice, so the screen is drawn afresh and its composer takes focus
   * (FLOW-NEW-SESSION-R01).
   */
  composing?: { projectId?: string; key: number };
  /** The first message on its way, or why it could not start a session. */
  starting?: Sending;
  /** The session just started, until the window follows it: the screen stays, sending, so the stage never blinks. */
  opening?: string;
  /** A session kept alive whose first message did not go through: its composer shows why, and the text if it is to be sent again. */
  firstSend?: { id: string; text?: string; message: string };
  /** The last `composing.key` given. */
  keys: number;
}

export type StageAction =
  | { type: "newSession"; projectId?: string }
  | { type: "select"; id: string }
  /** With no session chosen, the newest: a reopened window shows the session that was running when it closed. */
  | { type: "showNewest"; id: string }
  | { type: "sending" }
  | { type: "failed"; message: string }
  | { type: "started"; id: string; text: string; firstSend?: FirstSendFailure }
  /** The window now follows session `id`, so its transcript can show. */
  | { type: "followed"; id: string };

export const initialStage: StageState = { keys: 0 };

/** Whether a first message is on its way, during which the stage stays where it is. */
export const isStarting = (state: StageState) => state.starting?.state === "sending";

export function stage(state: StageState, action: StageAction): StageState {
  switch (action.type) {
    case "newSession":
      // A start under way finishes on the screen it began on (its failure or its session belongs there).
      if (isStarting(state)) return state;
      return { ...state, composing: { projectId: action.projectId, key: state.keys + 1 }, keys: state.keys + 1, starting: undefined };
    case "select":
      // Leaving the screen leaves nothing behind: nothing was created (FLOW-NEW-SESSION-R03).
      if (isStarting(state)) return state;
      return { ...state, activeId: action.id, composing: undefined, starting: undefined, firstSend: undefined };
    case "showNewest":
      return state.activeId === undefined ? { ...state, activeId: action.id } : state;
    case "sending":
      return { ...state, starting: { state: "sending" } };
    case "failed":
      // The screen stays as it is, so its composer keeps the message (FLOW-NEW-SESSION-R08).
      return { ...state, starting: { state: "failed", message: action.message } };
    case "started": {
      const firstSend = action.firstSend && {
        id: action.id,
        message: action.firstSend.message,
        ...(action.firstSend.giveBack ? { text: action.text } : {}),
      };
      // The session becomes the active one (FLOW-NEW-SESSION-R09); the screen stays until the window follows it.
      return { ...state, activeId: action.id, opening: action.id, firstSend };
    }
    case "followed":
      return state.opening === action.id ? { ...state, opening: undefined, composing: undefined, starting: undefined } : state;
  }
}
