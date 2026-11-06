// Which sessions are unseen (LAY-SHELL, The rail: Unseen; docs/design/RAIL.md, A tile: the title): a session that is
// not the active one and that Core reports as idle after working, until the user makes it active. The interface
// keeps this in memory only, so nothing survives the window closing.

import type { Session } from "../protocol/core-api.gen";

export interface Unseen {
  /** The unseen sessions' ids. */
  ids: ReadonlySet<string>;
  /** The sessions that were working when Core last reported them, to tell "idle after working" from plain idle. */
  working: ReadonlySet<string>;
}

export const noneUnseen: Unseen = { ids: new Set(), working: new Set() };

// Working is a running turn with nothing waiting on the user; a session with a pending request needs the user,
// which its own state says, so it is neither working nor idle here.
const isWorking = (s: Session) => s.tile.state === "running" && s.tile.pending.length === 0;
// A string, not the protocol's union: a state this interface doesn't know is never idle, so never marks a session.
const isIdle = (s: Session) => {
  const state: string = s.tile.state;
  return (state === "idle" || state === "waiting_user") && s.tile.pending.length === 0;
};

/**
 * The unseen sessions after Core reports `sessions` with `activeId` active. A session becomes unseen only when it
 * goes from working to idle while not active; the active session is never unseen, and one that leaves the list is
 * forgotten. Any other change (needing the user, failing, disconnecting, ending) leaves the mark as it was.
 */
export function unseenAfter(prev: Unseen, sessions: readonly Session[], activeId: string | undefined): Unseen {
  const ids = new Set<string>();
  const working = new Set<string>();
  for (const s of sessions) {
    if (isWorking(s)) working.add(s.id);
    if (s.id === activeId) continue;
    if (prev.ids.has(s.id) || (prev.working.has(s.id) && isIdle(s))) ids.add(s.id);
  }
  return { ids, working };
}
