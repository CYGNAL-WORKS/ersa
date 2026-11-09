// What the stage shows and how it changes: the active session's transcript, the new-session screen while the user
// has chosen New session or has no session (LAY-SHELL, FLOW-NEW-SESSION), or the sessions page over either. Kept free of React so the tests drive each
// transition; the window holds it with `useReducer`.

import type { Attached } from "./attachments";
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
  /** The session the window follows, as it last said. */
  following?: string;
  /**
   * A session kept alive whose first message did not go through: its composer shows why, and the text and attachments
   * if they are to be sent again, the one Core refused marked (FLOW-SEND-MESSAGE-R10).
   */
  firstSend?: { id: string; text?: string; attachments?: Attached[]; sending: Exclude<Sending, { state: "sending" }> };
  /** The last `composing.key` given. */
  keys: number;
  /**
   * The sessions page shows in place of the transcript or the new-session screen (FLOW-ORGANIZE-SESSIONS, step 9). The
   * rest of this state stands behind it: the active session stays active (R20).
   */
  sessionsPage?: boolean;
}

export type StageAction =
  | { type: "newSession"; projectId?: string }
  | { type: "allSessions" }
  /** The active session was archived or deleted: the new-session screen takes its place, behind the sessions page if it shows. */
  | { type: "activeLeft" }
  | { type: "select"; id: string }
  /** With no session chosen, the newest: a reopened window shows the session that was running when it closed. */
  | { type: "showNewest"; id: string }
  | { type: "sending" }
  | { type: "failed"; message: string }
  /** Stop ended the start before the message reached the agent: the screen stays, its message with it, and says nothing. */
  | { type: "stopped" }
  | { type: "started"; id: string; text: string; attachments?: readonly Attached[]; firstSend?: FirstSendFailure }
  /** The window now follows session `id`, so its transcript can show. */
  | { type: "followed"; id: string };

export const initialStage: StageState = { keys: 0 };

/** Whether a first message is on its way, during which the stage stays where it is. */
export const isStarting = (state: StageState) => state.starting?.state === "sending";

/**
 * Whether the stage shows the new-session screen when no session's transcript is shown: while the user has chosen
 * New session, while a first message is on its way or has just failed, and otherwise with no session while Core is
 * connected. The first two hold through a lost connection, so the screen, and the message in its composer, stay
 * (FLOW-NEW-SESSION-R08).
 */
export function showsScreen(state: Pick<StageState, "composing" | "starting" | "activeId">, connected: boolean): boolean {
  return state.composing !== undefined || state.starting !== undefined || (connected && state.activeId === undefined);
}

/** The new-session screen, drawn afresh. */
const compose = (state: StageState, projectId?: string): StageState => ({ ...state, composing: { projectId, key: state.keys + 1 }, keys: state.keys + 1, starting: undefined });

export function stage(state: StageState, action: StageAction): StageState {
  switch (action.type) {
    case "newSession":
      // A start under way finishes on the screen it began on (its failure or its session belongs there).
      if (isStarting(state)) return state;
      return { ...compose(state, action.projectId), sessionsPage: false };
    case "activeLeft":
      return isStarting(state) ? state : compose(state);
    case "allSessions":
      // Likewise: the screen with a start under way stays until it finishes.
      return isStarting(state) ? state : { ...state, sessionsPage: true };
    case "select":
      // Leaving the screen leaves nothing behind: nothing was created (FLOW-NEW-SESSION-R03).
      if (isStarting(state)) return state;
      // Back to the same session from the sessions page, its composer is as it was: a first message's refusal stays.
      return {
        ...state,
        activeId: action.id,
        composing: undefined,
        starting: undefined,
        firstSend: state.sessionsPage && !state.composing && action.id === state.activeId ? state.firstSend : undefined,
        sessionsPage: false,
      };
    case "showNewest":
      // Not while a start is on its way or has just failed: the newest session may be the one being started, and
      // the screen, its failure and the message in it stay until the user acts (FLOW-NEW-SESSION-R08).
      return state.activeId === undefined && state.starting === undefined ? { ...state, activeId: action.id } : state;
    case "sending":
      return { ...state, starting: { state: "sending" } };
    case "stopped":
      return { ...state, starting: undefined };
    case "failed":
      // The screen stays as it is, so its composer keeps the message (FLOW-NEW-SESSION-R08).
      return { ...state, starting: { state: "failed", message: action.message } };
    case "started": {
      const failed = action.firstSend;
      const refused = failed?.sending.state === "failed" ? failed.sending : undefined;
      const firstSend = failed && {
        id: action.id,
        sending: failed.sending,
        // The attachment Core refused is marked with why, which shows beside it rather than in the status line.
        ...(failed.giveBack
          ? { text: action.text, attachments: action.attachments?.map((a, i) => (refused && i === refused.index ? { ...a, problem: refused.message } : a)) }
          : {}),
      };
      // The session becomes the active one (FLOW-NEW-SESSION-R12); the screen stays until the window follows it,
      // and goes now when the window already does.
      if (state.following === action.id) return { ...state, activeId: action.id, opening: undefined, composing: undefined, starting: undefined, firstSend };
      return { ...state, activeId: action.id, opening: action.id, firstSend };
    }
    case "followed":
      return state.opening === action.id
        ? { ...state, following: action.id, opening: undefined, composing: undefined, starting: undefined }
        : { ...state, following: action.id };
  }
}
