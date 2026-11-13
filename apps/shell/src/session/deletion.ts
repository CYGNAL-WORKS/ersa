// Deleting a session from its tile (FLOW-ARCHIVE-DELETE-SESSION, steps 4 to 6, Variations): when Delete is
// unavailable, what the delete question says, and the round trips behind it. The question and the dialog only lay
// out what is decided here, so every decision is made, and tested, without drawing anything.

import { t } from "../catalog";
import { type CoreClient, describeError, refusalOf } from "../core/client";
import { inTurn } from "../core/shell";
import type { Session, Workspace, WorkspaceChanges } from "../protocol/core-api.gen";

/**
 * Why Delete can't be chosen now, or undefined when it can. A session starting, working or waiting on the user is
 * refused by Core, so the menu says to stop it first (FLOW-ARCHIVE-DELETE-SESSION-R05).
 */
export function deleteUnavailable(session: Session, offline: boolean): string | undefined {
  if (offline) return t("rail.tile.offlineReason");
  // Core holds a running deletion's workspace, and refuses another until it ends.
  if (session.deleting === "running") return t("rail.delete.deletingReason");
  if (inTurn(session)) return t("rail.delete.busyReason");
  return undefined;
}

/** What goes with the session's conversation, as the question says it (FLOW-ARCHIVE-DELETE-SESSION, step 4). */
export type WhatGoes =
  /** The session's workspace is the project's own folder, or another session uses it: its files stay where they are. */
  | { kind: "filesStay"; path: string }
  /** The session is alone in an isolated workspace: its folder goes, and its branch unless the branch is kept. */
  | {
      kind: "folderGoes";
      /** The branch by name; empty when the workspace has none (a folder outside version control). */
      branch: string;
      /** The branch holds commits no other branch or remote holds, so it is kept. */
      branchKept: boolean;
      /** What goes with the folder that exists nowhere else; all zero, nothing is lost. */
      uncommitted: number;
      ignored: number;
      stranded: number;
      /** Git has lost the workspace's record, so commits made on a detached checkout can't be counted (R10). */
      unrecorded: boolean;
    };

/**
 * What goes, from the session's workspace and, for an isolated one, its `workspace.changes` answer. `marked`: the
 * session is already marked deleting, after a delete that failed part way.
 */
export function whatGoes(workspace: Workspace, changes: WorkspaceChanges | undefined, marked = false): WhatGoes {
  // `sessions` leaves out sessions marked deleting, so it counts this one only while it isn't. Any other session
  // using the folder keeps it.
  const others = changes ? changes.sessions - (marked ? 0 : 1) : 0;
  if (workspace.kind === "root" || !changes || others > 0) return { kind: "filesStay", path: workspace.path };
  return {
    kind: "folderGoes",
    branch: workspace.vcs_ref,
    // Of the unshared commits, those the branch holds are the ones not stranded; any of them keeps the branch.
    branchKept: changes.unshared - changes.stranded > 0,
    uncommitted: changes.uncommitted,
    ignored: changes.ignored,
    // With git's record gone, stranded commits can't be counted; the question says so instead of a count.
    stranded: changes.unrecorded ? 0 : changes.stranded,
    unrecorded: changes.unrecorded,
  };
}

/** Whether confirming loses work that exists nowhere else, so the confirming button reads as deleting it. */
export const losesWork = (goes: WhatGoes): boolean =>
  goes.kind === "folderGoes" && (goes.uncommitted > 0 || goes.ignored > 0 || goes.stranded > 0 || goes.unrecorded);

/** Where the delete question stands. */
export type DeletePhase =
  /** Reading what deleting would lose. */
  | { kind: "reading" }
  /** Asking. `changed`: Ersa asked once already, and what would be lost has changed since (R08). */
  | { kind: "asking"; goes: WhatGoes; changed: boolean }
  /** Confirmed, and deleting. */
  | { kind: "deleting"; goes: WhatGoes }
  /** Reading or deleting failed, for this reason; Try again reads what is left and asks again (R07). */
  | { kind: "failed"; reason: string }
  /** Deleted: the session is gone. */
  | { kind: "deleted" };

/**
 * The delete question's round trips with Core, for one session. `onPhase` hears each phase as it begins, starting
 * with `reading` from `start`.
 */
export class Deletion {
  private token: string | undefined;
  // Whether Core has marked the session deleting, as it does once a delete fails part way: `workspace.changes` then
  // leaves it out of the sessions it counts.
  private marked = false;
  private phase: DeletePhase = { kind: "reading" };

  constructor(
    private readonly client: Pick<CoreClient, "call">,
    private readonly sessionId: string,
    private readonly workspace: Workspace,
    private readonly onPhase: (phase: DeletePhase) => void,
  ) {}

  /** Reads what deleting would lose and asks. Try again after a failure is the same: it asks again with what is left. */
  async start(): Promise<void> {
    this.set({ kind: "reading" });
    // A root workspace's files are never deleted, so there is nothing to count (Core refuses to).
    if (this.workspace.kind === "root") {
      this.token = undefined;
      this.set({ kind: "asking", goes: whatGoes(this.workspace, undefined), changed: false });
      return;
    }
    try {
      const [changes, listed] = await Promise.all([
        this.client.call("workspace.changes", { id: this.workspace.id }),
        // Unread, it is taken as unmarked, which can only overstate what goes, never hide it.
        this.client.call("session.list").catch(() => []),
      ]);
      this.marked = listed.some((s) => s.id === this.sessionId && s.deleting !== undefined);
      this.ask(changes, false);
    } catch (error) {
      this.set({ kind: "failed", reason: describeError(error) });
    }
  }

  /** The user confirmed: deletes with the token of what they were shown. */
  async confirm(): Promise<void> {
    if (this.phase.kind !== "asking") return;
    this.set({ kind: "deleting", goes: this.phase.goes });
    try {
      await this.client.call("session.delete", this.token === undefined ? { id: this.sessionId } : { id: this.sessionId, token: this.token });
      this.set({ kind: "deleted" });
    } catch (error) {
      // What would be lost changed since the question: Core answers with what it is now, and Ersa asks again (R08).
      const changes = staleChanges(error);
      if (changes) this.ask(changes, true);
      else this.set({ kind: "failed", reason: reasonOf(error) });
    }
  }

  private ask(changes: WorkspaceChanges, changed: boolean) {
    this.token = changes.token;
    this.set({ kind: "asking", goes: whatGoes(this.workspace, changes, this.marked), changed });
  }

  private set(phase: DeletePhase) {
    this.phase = phase;
    this.onPhase(phase);
  }
}

/** Why a delete failed, in the user's terms: Core's bare `busy` becomes a sentence, any other reason is Core's own. */
function reasonOf(error: unknown): string {
  const data = refusalOf(error);
  return data?.kind === "invalid_state" && data.detail === "busy" ? String(t("rail.delete.busyError")) : describeError(error);
}

/**
 * The new `workspace.changes` when `session.delete` was refused for a stale token: `invalid_state` with the object as
 * JSON text in `detail` (PROTO-CORE-API-R53). Any other refusal, `busy` or a process named among them, is not one.
 */
function staleChanges(error: unknown): WorkspaceChanges | undefined {
  const data = refusalOf(error);
  if (data?.kind !== "invalid_state" || !data.detail?.startsWith("{")) return undefined;
  try {
    const changes: unknown = JSON.parse(data.detail);
    return typeof changes === "object" && changes !== null && typeof (changes as WorkspaceChanges).token === "string"
      ? (changes as WorkspaceChanges)
      : undefined;
  } catch {
    return undefined;
  }
}
