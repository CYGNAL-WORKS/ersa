// What a session's tile in the rail shows, from the session as Core lists it (LAY-SHELL, The rail: Tiles, State,
// Needs you, Context; docs/design/RAIL.md, A tile). The component only lays this out, so every decision about a
// state, a tone or a phrase is made, and tested, here.

import { t } from "../catalog";
import type { BackendInfo, Session } from "../protocol/core-api.gen";


type Tile = Session["tile"];

/** The mark at a tile's trailing edge. Idle, ended and an unknown state have none; an archived session has its own. */
export type TileMark = "starting" | "working" | "needsYou" | "disconnected" | "error" | "archived";

/** The tone a mark is drawn in; a needs-you mark's phrase takes it too. */
export type TileTone = "accent" | "caution" | "danger" | "faint" | "muted";

export interface TileView {
  title: string;
  /**
   * The title reads `ink-muted` rather than `ink`: the session is idle or has ended, or is archived, and is not
   * unseen.
   */
  titleMuted: boolean;
  /**
   * The title reads at the heading weight in `ink` (LAY-SHELL, Unseen): Core reports the session unseen and it is not
   * the active one. An unseen title is never muted.
   */
  titleUnseen: boolean;
  /** The backend's name, the accessible name of its icon. */
  backend: string;
  mark?: TileMark;
  tone?: TileTone;
  /** The mark's accessible name, where line two doesn't already say it. */
  markLabel?: string;
  /** Line two's words, before the cost. */
  phrase?: string;
  /** The cost as reported, formatted. */
  cost?: string;
  /** The context ring's share used, from 0 to 1; absent until both the tokens and a window are reported, while the ring is empty. */
  ring?: number;
}

// The cost reads in dollars to the cent, in the locale the catalog is in.
const dollars = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD" });

/** A session's title as Ersa shows it. */
export const titleOf = (session: Pick<Session, "title">) => session.title || t("rail.tile.untitledLabel");

/** What the window knows of a session that Core doesn't list. */
export interface TileContext {
  /** A fork of it is on its way. */
  forking?: boolean;
  /** It is the active session: its title never reads unseen, even in the render that makes it active. */
  active?: boolean;
}

/** What a session's tile shows. `backends` gives each host's backends, to name a session's backend by its own name. */
export function tileOf(session: Session, backends: Readonly<Record<string, readonly BackendInfo[]>> = {}, { forking = false, active = false }: TileContext = {}): TileView {
  const view = viewOf(session, backends, forking);
  // Unseen beats muted: a session that finished while the user looked elsewhere reads strong whatever rests it.
  const titleUnseen = session.unseen && !active;
  return { ...view, titleUnseen, titleMuted: view.titleMuted && !titleUnseen };
}

function viewOf(session: Session, backends: Readonly<Record<string, readonly BackendInfo[]>>, forking: boolean): Omit<TileView, "titleUnseen"> {
  const { tile } = session;
  const cost = tile.cost_usd_total;
  const base = {
    title: titleOf(session),
    backend: backends[session.host_id]?.find((b) => b.name === session.backend)?.label ?? session.backend,
    cost: cost !== undefined && Number.isFinite(cost) ? dollars.format(cost) : undefined,
    ring: contextUsed(tile),
  };
  const deleting = deletingOf(session);
  // Being deleted takes precedence over archived (LAY-SHELL, Being deleted).
  if (deleting) return { ...base, ...deleting };
  // A fork of it on its way, which Core may take seconds to start, reads as a deletion under way does, in place, so
  // nothing enters the rail until the fork's own tile does (FLOW-FORK-SESSION, step 2). `forking` is the window's to
  // say: Core lists nothing for it.
  const state = stateOf(tile);
  // Only over a state with no mark of its own (idle, ended): the original keeps its state (step 4), so a turn started
  // in it meanwhile, a request for the user or an error still shows.
  if (forking && !state.mark) return { ...base, titleMuted: false, mark: "starting", tone: "muted", phrase: t("rail.tile.forkingProgress") };
  // An archived session's tile reads quieter and carries the archive mark in place of its state's (LAY-SHELL,
  // Archived); line two still says its state.
  if (session.archived) return { ...base, ...state, titleMuted: true, mark: "archived", tone: undefined, markLabel: t("rail.tile.archivedStatus") };
  return { ...base, ...state };
}

type StateView = Pick<TileView, "titleMuted" | "mark" | "tone" | "markLabel" | "phrase">;

// A session being deleted, or whose deletion failed, says so before every other mark and phrase (LAY-SHELL, Being
// deleted): the starting arc in ink-muted while Core works on it, the error mark once it failed.
function deletingOf(session: Session): StateView | undefined {
  if (session.deleting === "running") return { titleMuted: false, mark: "starting", tone: "muted", phrase: t("rail.tile.deletingStatus") };
  if (session.deleting === "failed") return { titleMuted: false, mark: "error", tone: "danger", phrase: t("rail.tile.deleteFailedStatus") };
  return undefined;
}

/**
 * The tone of what a session with pending requests wants: only questions are the accent; a permission request, or a
 * kind this interface doesn't know, is caution.
 */
export function wantTone(pending: Tile["pending"]): "accent" | "caution" {
  return pending.every((p) => p.kind === "question") ? "accent" : "caution";
}

/**
 * What a session is doing, as its tile and the unseen mark both read it (LAY-SHELL, State; Unseen). The pending
 * requests decide before the state does: a session with one needs the user whatever Core reports, so it is neither
 * working nor idle, and one reported as waiting on the user with none is idle. A state this interface doesn't know is
 * neither.
 */
export function activityOf(tile: Tile): "needsYou" | "working" | "idle" | undefined {
  if (tile.pending.length > 0) return "needsYou";
  // A string, not the protocol's union: a newer Core may report a state this interface doesn't know.
  const state: string = tile.state;
  if (state === "running") return "working";
  if (state === "idle" || state === "waiting_user") return "idle";
  return undefined;
}

function stateOf(tile: Tile): StateView {
  const { pending } = tile;
  const activity = activityOf(tile);
  if (activity === "idle") return { titleMuted: true };
  if (activity === "working") return { titleMuted: false, mark: "working", tone: "accent", markLabel: t("rail.tile.workingStatus") };
  if (activity === "needsYou") {
    const tone = wantTone(pending);
    const only = pending.length === 1 ? pending[0]?.kind : undefined;
    const phrase =
      pending.length > 1
        ? t("rail.tile.needsYouCountWant", { count: pending.length })
        : only === "permission"
          ? t("rail.tile.permissionWant")
          : only === "question"
            ? t("rail.tile.questionWant")
            : t("rail.tile.needsYouWant");
    return { titleMuted: false, mark: "needsYou", tone, markLabel: phrase, phrase };
  }
  const state: string = tile.state;
  switch (state) {
    case "starting":
      return { titleMuted: false, mark: "starting", tone: "accent", phrase: t("rail.tile.startingStatus") };
    case "disconnected":
      return { titleMuted: false, mark: "disconnected", tone: "faint", phrase: t("rail.tile.disconnectedStatus") };
    case "error":
      return { titleMuted: false, mark: "error", tone: "danger", phrase: t("rail.tile.errorStatus") };
    case "ended":
      return { titleMuted: true, phrase: t("rail.tile.endedStatus") };
    default:
      return { titleMuted: false, phrase: t("rail.tile.unknownStateStatus") };
  }
}

/** The share of the context window used, from 0 to 1: absent until the tokens and a window above zero are reported. */
export function contextUsed(tile: Tile): number | undefined {
  const { context_tokens: tokens, context_window: window } = tile;
  if (tokens === undefined || window === undefined || !Number.isFinite(tokens) || !Number.isFinite(window) || window <= 0) return undefined;
  // Never more than full, and never less than empty.
  return Math.min(Math.max(tokens / window, 0), 1);
}

/** The context ring's band: quiet, then caution, then danger as the window fills. */
export type RingBand = "quiet" | "caution" | "danger";

/**
 * The context ring's band (LAY-SHELL, Context), on a tile and in the session controls: quiet up to half, caution past
 * half, danger past four fifths. It is read in whole percent, as the ring's readout and name round it, so the colour
 * always agrees with the figure: what reads 50 percent is quiet and what reads 51 percent is caution. Absent with no reading.
 */
export function ringBand(used: number | undefined): RingBand | undefined {
  if (used === undefined) return undefined;
  const percent = Math.round(used * 100);
  return percent > 80 ? "danger" : percent > 50 ? "caution" : "quiet";
}
