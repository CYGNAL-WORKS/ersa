// Ersa's own unseen mark (FLOW-ORGANIZE-SESSIONS, Unseen; LAY-SHELL, Unseen): a session that goes idle after working
// while it is not the active one becomes unseen, and the mark goes when the user makes it active. Core stores the
// mark and lists it (PROTO-CORE-API-R78), so it survives a restart of Ersa and of Core (FLOW-ORGANIZE-SESSIONS-R25);
// this only works out what to tell Core, from each list Core reports. Core never sets the mark by itself.

import type { Session } from "../protocol/core-api.gen";
import { activityOf } from "./tile";

/** A change of the unseen mark to send to Core with `session.arrange`. */
export interface UnseenWrite {
  id: string;
  unseen: boolean;
}

/**
 * What Ersa tells Core after Core reports `sessions` with `activeId` active, given the sessions that were working
 * in the report before (`working`). A session not active that goes from working to idle is marked unseen, unless it
 * already is; the active session is marked seen if Core reports it unseen. A session with a change already on its way
 * (`sending`) gets no other, so a list read before that change answers can't send it twice, and one Core refused
 * (`refused`: what was asked) is not asked the same again. Needing the user, failing, disconnecting or ending marks
 * nothing.
 *
 * Returns the sessions working now, for the next report, and the changes to send.
 */
export function unseenWrites(
  working: ReadonlySet<string>,
  sessions: readonly Session[],
  activeId: string | undefined,
  sending: ReadonlySet<string>,
  refused: ReadonlyMap<string, boolean> = new Map(),
): { working: ReadonlySet<string>; writes: UnseenWrite[] } {
  const now = new Set<string>();
  const writes: UnseenWrite[] = [];
  for (const s of sessions) {
    const activity = activityOf(s.tile);
    if (activity === "working") now.add(s.id);
    if (sending.has(s.id)) {
      // Decided once its change has answered: a turn that ended meanwhile still marks it from the next list.
      if (working.has(s.id)) now.add(s.id);
      continue;
    }
    if (s.id === activeId) {
      if (s.unseen) writes.push({ id: s.id, unseen: false });
    } else if (working.has(s.id) && activity === "idle" && !s.unseen) {
      writes.push({ id: s.id, unseen: true });
    }
  }
  return { working: now, writes: writes.filter((w) => refused.get(w.id) !== w.unseen) };
}
