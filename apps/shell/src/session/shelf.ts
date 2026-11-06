// The shelf (FLOW-APPROVE-PERMISSION; LAY-SHELL, The shelf): which session's shelf is open, what it shows, and
// following that session's events while it is open. Kept free of React, so every decision is made, and tested,
// here; the components only lay it out.

import type { Envelope, PermissionRequested, QuestionAsked } from "../protocol/agent-events.gen";
import type { Session } from "../protocol/core-api.gen";
import { EARLIER, type Follower, type SessionView, follow } from "./follow";
import type { SharedSubscriptions } from "./subscriptions";
import { type Item, isOpen } from "./transcript";

type Pending = Session["tile"]["pending"][number];

// --- Which shelf is open -------------------------------------------------------------------------------------

/** What changes which shelf is open. */
export type ShelfChange =
  /** The user chose a tile's chevron. */
  | { type: "chevron"; id: string; activeId?: string }
  /** The user closed a session's shelf without its chevron: Escape, or a choice or focus elsewhere (R06, R07). */
  | { type: "close"; id: string }
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
    case "close":
      // Only the shelf named: a close that comes after another shelf opened leaves that one open.
      return change.id === open ? undefined : open;
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
 *
 * `shown` is the requests this shelf has shown since it opened. Its place is "n of m" (R09): n counts those no
 * longer waiting, plus one, and m adds the requests still waiting after the shown one. A request whose answer
 * arrived as an event no longer waits, though the tile may still list it until Core lists its sessions again.
 */
export function shelfOf(view: SessionView | undefined, pending: readonly Pending[], shown: ReadonlySet<string> = new Set()): ShelfShows {
  if (!view) return { state: "loading" };
  // Reading failed, including a gap that subscribing again did not fill: what is held may be out of date.
  if (view.stream.state === "failed") return { state: "failed", message: view.stream.message };
  // Subscribing, or waiting for a connection before anything was read.
  if (view.stream.state === "subscribing" || (view.stream.state === "offline" && view.transcript.lastSeq < 0)) return { state: "loading" };

  const { items } = view.transcript;
  // Answered, by its event: no longer waiting, whatever the tile still lists.
  const settled = new Set(
    items.flatMap((item) => {
      if (item.kind === "permission" && !isOpen(item)) return [item.request.request_id];
      if (item.kind === "question" && item.answered) return [item.requestId];
      return [];
    }),
  );
  const stillPending = pending.filter((p) => !settled.has(p.request_id));
  const waiting = new Map(stillPending.map((p) => [p.request_id, p.kind as string]));
  type Request = Extract<Item, { kind: "permission" | "question" }>;
  const showable = items.filter((item): item is Request => {
    if (item.kind === "permission") return isOpen(item) && waiting.get(item.request.request_id) === "permission";
    if (item.kind === "question") return !item.answered && waiting.get(item.requestId) === "question";
    return false;
  });
  const unshowable = stillPending.length - showable.length;
  const answered = [...shown].filter((id) => !waiting.has(id)).length;
  const m = answered + stillPending.length;
  const place = m > 1 ? { n: answered + 1, m } : undefined;
  // Every request it knew of is answered, though the tile still lists them: nothing is left to show, and the shelf
  // closes once Core lists its sessions again.
  if (stillPending.length === 0) return { state: "loading" };
  const oldest = showable[0];
  if (!oldest || (unshowable > 0 && missedEarlier(view))) return { state: "unknown", place };
  if (oldest.kind === "permission") return { state: "permission", request: oldest.request, place };
  return { state: "question", requestId: oldest.requestId, questions: oldest.questions, place };
}

/** The request the shelf shows, if it shows one. */
export function shownRequest(shows: ShelfShows): string | undefined {
  if (shows.state === "permission") return shows.request.request_id;
  if (shows.state === "question") return shows.requestId;
  return undefined;
}

/** The requests a session's shelf has shown since it opened, which count toward its place (R09). */
export interface ShelfShown {
  id: string;
  requests: ReadonlySet<string>;
}

/**
 * The shown requests after the open shelf (`id`) shows `request`: a new shelf starts afresh, and a closed one keeps
 * nothing. Returns `shown` itself when nothing changes.
 */
export function shownAfter(shown: ShelfShown | undefined, id: string | undefined, request: string | undefined): ShelfShown | undefined {
  if (id === undefined) return undefined;
  const requests = shown?.id === id ? shown.requests : new Set<string>();
  if (request === undefined || requests.has(request)) return shown?.id === id ? shown : { id, requests };
  return { id, requests: new Set([...requests, request]) };
}

/** The events held start after a gap: Core let go of the session's earliest events. */
const missedEarlier = (view: SessionView) => view.transcript.items[0]?.key.startsWith(EARLIER) ?? false;

// --- Following the shelf's session ---------------------------------------------------------------------------

export interface ShelfFollowing {
  /** Follows `id`'s events, or none: the open shelf's session. Following another stops the last. */
  follow(id: string | undefined, connected: boolean): void;
  receive(envelope: Envelope): void;
  /** Subscribes on a new connection, and again after reading failed (R21, Retry). */
  attached(): void;
  detached(): void;
}

/**
 * Follows the open shelf's session while it is open, and stops when it closes (FLOW-APPROVE-PERMISSION, Intent).
 * Its clients share Core's subscriptions with the transcript's, so a session both follow stays subscribed until
 * both let go.
 */
export function shelfFollowing(clients: SharedSubscriptions, update: (view: SessionView | undefined) => void): ShelfFollowing {
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
      const following = follow(clients(), id, (view) => currentId === id && update(view));
      current = following;
      if (connected) following.attached();
      else following.detached();
    },
    receive: (envelope) => current?.receive(envelope),
    attached: () => current?.attached(),
    detached: () => current?.detached(),
  };
}
