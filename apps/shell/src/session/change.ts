// Changing a running session's model, effort or permission mode from its session controls (FLOW-NEW-SESSION-R30).
// Kept free of React so the tests drive it with a fake client.

import { t } from "../catalog";
import { type CoreClient, describeError } from "../core/client";
import type { Session } from "../protocol/core-api.gen";

/** What a running session's controls can change: the chips that open under it. */
const CHANGEABLE = ["model", "effort", "permission_mode"] as const;
export type Changeable = (typeof CHANGEABLE)[number];

export const isChangeable = (key: string): key is Changeable => (CHANGEABLE as readonly string[]).includes(key);

/**
 * A session's changes: the chips whose change is on its way to Core, each on its own, and why the last refused one
 * was refused, which the status line says until the user sends or opens a chip.
 */
export interface Changes {
  sending: readonly Changeable[];
  refused?: string;
}

export const NO_CHANGES: Changes = { sending: [] };

/** A change from `key`'s chip is on its way. */
export const changeSent = (c: Changes, key: Changeable): Changes => ({ ...c, sending: [...c.sending.filter((k) => k !== key), key] });

/** Core answered `key`'s change: accepted, or refused with why. Another change on its way, and its answer, stand. */
export const changeAnswered = (c: Changes, key: Changeable, refused?: string): Changes => ({
  sending: c.sending.filter((k) => k !== key),
  refused: refused ?? c.refused,
});

/** The user sent or opened a chip: the refusal's line goes; changes on their way stay, so their answers are shown. */
export const refusalSeen = (c: Changes): Changes => ({ sending: c.sending });

/**
 * Whether the session runs in Core, so a change reaches its backend: Core refuses one for a session that is not live
 * (PROTO-CORE-API, `session.set_model`). A session's state arrives with its tile, so that is the state read; one Core
 * has not run since it started is `ended`, and one whose backend exited with an error is `error`.
 */
export function runs(session: Pick<Session, "tile">): boolean {
  return session.tile.state !== "ended" && session.tile.state !== "error" && session.tile.state !== "disconnected";
}

/** The words before Core's reason for each refused change. */
const REFUSED = {
  model: () => t("sessionControls.change.modelError"),
  effort: () => t("sessionControls.change.effortError"),
  permission_mode: () => t("sessionControls.change.permissionModeError"),
} satisfies Record<Changeable, () => string>;

/** Core's reason as a sentence of its own after the catalog's: a capital first, a period last. */
function sentence(reason: string): string {
  const text = reason.trim();
  if (text === "") return text;
  const capital = text.charAt(0).toUpperCase() + text.slice(1);
  return /[.!?…]$/.test(capital) ? capital : `${capital}.`;
}

/** The fields of the session a change can move, as Core answers them: the backend's word on the new value. */
export type Changed = Pick<Session, "model" | "model_request" | "effort" | "permission_mode">;

/**
 * Asks Core to change `key` to `value` on session `id`. Resolves with the session's changed fields as Core lists them
 * once the backend has the change, or rejects with what the status line says: why, in the user's terms.
 */
export async function changeSession(client: Pick<CoreClient, "call">, id: string, key: Changeable, value: string): Promise<Changed> {
  try {
    const answer =
      key === "model"
        ? await client.call("session.set_model", { id, model: value })
        : key === "effort"
          ? await client.call("session.set_effort", { id, effort: value })
          : await client.call("session.set_permission_mode", { id, permission_mode: value });
    // Only what the change moves: the rest of the session, its tile above all, comes from `session.list` as ever.
    return { model: answer.model, model_request: answer.model_request, effort: answer.effort, permission_mode: answer.permission_mode };
  } catch (error) {
    throw new Error(`${REFUSED[key]()} ${sentence(describeError(error))}`);
  }
}
