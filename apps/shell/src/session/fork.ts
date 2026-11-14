// Fork on a session's menu (FLOW-FORK-SESSION): when the menu can already tell it is unavailable, and why. Core
// decides from the session's record (PROTO-CORE-API, `session.fork`); the listing tells only some of its refusals
// ahead, so the others are said beneath the tile once Core gives them.

import { t } from "../catalog";
import { inTurn } from "../core/shell";
import type { Session } from "../protocol/core-api.gen";
import type { Sending } from "./follow";

/**
 * Why Fork can't be chosen now, or undefined when it can (FLOW-FORK-SESSION-R03). `forking` is this session's fork on
 * its way, or the refusal of the last one, which does not hold Fork back: what refused it may have changed.
 */
export function forkUnavailable(session: Session, offline: boolean, forking?: Sending): string | undefined {
  if (offline) return t("rail.tile.offlineReason");
  // Core refuses a second fork of a session while the first is starting.
  if (forking?.state === "sending") return t("rail.tile.forkingProgress");
  // A reply running, a request waiting on the user, or a message on its way: Core refuses until the turn ends.
  if (inTurn(session) || session.sending !== undefined) return t("rail.tile.forkBusyReason");
  // Whether a reply has finished, and whether the last turn was interrupted or another process has the session open,
  // only Core can tell from the record: the listing says none of it (a missing last activity is no sign, since
  // sessions whose turns ended before Core kept it have none), so Core's refusal says those beneath the tile.
  return undefined;
}
