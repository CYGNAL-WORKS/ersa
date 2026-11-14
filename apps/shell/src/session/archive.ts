// Archive on a session's menu (FLOW-ARCHIVE-DELETE-SESSION, steps 1 and 3): when the menu can already tell it is
// unavailable, and why. The menu only lays out what is decided here, so the decision is tested without drawing.

import { t } from "../catalog";
import { inTurn } from "../core/shell";
import type { Session } from "../protocol/core-api.gen";
import type { Sending } from "./follow";

/**
 * Why Archive or Unarchive can't be chosen now, or undefined when it can. `archiving` is this session's archive or
 * unarchive on its way, or the refusal of the last one, which does not hold the item back.
 */
export function archiveUnavailable(session: Session, offline: boolean, archiving?: Sending): string | undefined {
  if (offline) return t("rail.tile.offlineReason");
  // One choice at a time, as Pin does, so a slow Core is not sent the same archive twice.
  if (archiving?.state === "sending") return t("rail.tile.savingProgress");
  // Core refuses to archive a session starting, working, waiting on the user or being sent a message
  // (FLOW-ARCHIVE-DELETE-SESSION-R11). An archived session is stopped, so Unarchive is never held back for this.
  if (!session.archived && (inTurn(session) || session.sending !== undefined)) return t("rail.tile.archiveBusyReason");
  return undefined;
}
