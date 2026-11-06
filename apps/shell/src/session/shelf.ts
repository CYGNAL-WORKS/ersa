// The shelf (FLOW-APPROVE-PERMISSION; LAY-SHELL, The shelf): which session's shelf is open, what it shows, and
// following that session's events while it is open. Kept free of React, so every decision is made, and tested,
// here; the components only lay it out.

import type { Envelope, PermissionRequested, QuestionAsked } from "../protocol/agent-events.gen";
import type { CoreClient } from "../core/client";
import type { Session } from "../protocol/core-api.gen";
import { EARLIER, type Follower, type SessionView, follow } from "./follow";
import { type Item, isOpen } from "./transcript";

type Pending = Session["tile"]["pending"][number];

// --- Which shelf is open -------------------------------------------------------------------------------------

/** What changes which shelf is open. */
export type ShelfChange =
  /** The user chose a tile's chevron. */
  | { type: "chevron"; id: string; activeId?: string }
  /** The sessions as Core lists them now, and the active session. */
  | { type: "sessions"; sessions: readonly Session[]; activeId?: string };

/**
 * The session whose shelf is open after `change`, or none. Only choosing a chevron opens one (R02), and only for a
 * session that is not the active one (R26); opening one closes any other, since this holds a single id (R03).
 * Choosing the open shelf's chevron again closes it. Nothing here changes the active session (R01).
 */
export function shelfAfter(open: string | undefined, change: ShelfChange): string | undefined {
  switch (change.type) {
    case "chevron":
      if (change.id === change.activeId) return open;
      return change.id === open ? undefined : change.id;
    case "sessions": {
      if (open === undefined) return undefined;
      // Making the session active closes its shelf (R26); so does it having nothing left to ask, or going.
      if (open === change.activeId) return undefined;
      const session = change.sessions.find((s) => s.id === open);
      return session && session.tile.pending.length > 0 ? open : undefined;
    }
  }
}

// --- What the shelf shows ------------------------------------------------------------------------------------

/** The shelf's place among the session's requests, "n of m" (`docs/design/RAIL.md`, The heading line). */
export interface Place {
  n: number;
  m: number;
}

export type ShelfShows =
  /** Reading the session's events (R21). */
  | { state: "loading" }
  /** Reading them failed, and why (R21). */
  | { state: "failed"; message: string }
  /** A request of a kind Ersa doesn't know, or whose event Core no longer holds (R25). */
  | { state: "unknown"; place?: Place }
  | { state: "permission"; request: PermissionRequested; place?: Place }
  | { state: "question"; requestId: string; questions: QuestionAsked["questions"]; place?: Place };

/** The tone of the shelf and of its tile while open (`docs/design/RAIL.md`, The shelf). */
export function shelfTone(shows: ShelfShows): "accent" | "caution" {
  return shows.state === "question" ? "accent" : "caution";
}

/**
 * What the shelf shows for a session: its oldest pending request, the one whose event came first (R01), read
 * from the session's events. `pending` is the tile's list, which says what waits; the events say what each is.
 *
 * A pending request with no event among those held, or of a kind this window doesn't know, can't be shown (R25).
 * Where it falls in the order is unknown, so it counts as oldest only when the events held start after a gap
 * (Core let go of the earliest ones, where its event must be); otherwise the requests that can be shown go first.
 */
export function shelfOf(view: SessionView | undefined, pending: readonly Pending[]): ShelfShows {
  if (!view || (view.stream.state === "subscribing" && view.transcript.lastSeq < 0)) return { state: "loading" };
  // A gap is only a failure once subscribing again didn't fill it; until then what is held still reads.
  if (view.stream.state === "failed" && !view.stream.gap) return { state: "failed", message: view.stream.message };
  if (view.stream.state === "subscribing") return { state: "loading" };

  const waiting = new Map(pending.map((p) => [p.request_id, p.kind as string]));
  const shown: Item[] = view.transcript.items.filter((item) => {
    if (item.kind === "permission") return isOpen(item) && waiting.get(item.request.request_id) === "permission";
    if (item.kind === "question") return !item.answered && waiting.get(requestIdOf(item)) === "question";
    return false;
  });
  const unshowable = pending.length - shown.length;
  const m = pending.length;
  const place = m > 1 ? { n: 1, m } : undefined;
  const oldest = shown[0];
  if (!oldest || (unshowable > 0 && missedEarlier(view))) return { state: "unknown", place };
  if (oldest.kind === "permission") return { state: "permission", request: oldest.request, place };
  if (oldest.kind === "question") return { state: "question", requestId: requestIdOf(oldest), questions: oldest.questions, place };
  return { state: "unknown", place };
}

const requestIdOf = (item: Extract<Item, { kind: "question" }>) => item.key.slice("question:".length);

/** The events held start after a gap: Core let go of the session's earliest events. */
const missedEarlier = (view: SessionView) => view.transcript.items[0]?.key.startsWith(EARLIER) ?? false;

// --- Following the shelf's session ---------------------------------------------------------------------------

export interface ShelfFollowing {
  /** Follows `id`'s events, or none: the open shelf's session. Following another stops the last. */
  follow(id: string | undefined, connected: boolean): void;
  receive(envelope: Envelope): void;
  attached(): void;
  detached(): void;
  /** Subscribes again after reading failed (R21, Retry). */
  retry(): void;
}

/**
 * Follows the open shelf's session while it is open, and stops when it closes (FLOW-APPROVE-PERMISSION, Intent).
 * The shelf is never open for the active session, so this never shares or ends the transcript's subscription.
 */
export function shelfFollowing(client: CoreClient, update: (view: SessionView | undefined) => void): ShelfFollowing {
  let current: Follower | undefined;
  let currentId: string | undefined;
  return {
    follow(id, connected) {
      if (id === currentId) return;
      current?.stop();
      current = undefined;
      currentId = id;
      update(undefined);
      if (id === undefined) return;
      // Only the session followed now updates the shelf: a stopped follower is silent anyway, and this keeps it so.
      const following = follow(client, id, (view) => currentId === id && update(view));
      current = following;
      if (connected) following.attached();
      else following.detached();
    },
    receive: (envelope) => current?.receive(envelope),
    attached: () => current?.attached(),
    detached: () => current?.detached(),
    retry: () => current?.attached(),
  };
}
