import { useLayoutEffect, useRef, useState } from "react";
import { t } from "../catalog";
import type { Answer, Decision } from "../session/answers";
import { type ShelfShows, shelfTone } from "../session/shelf";
import type { TileView } from "../session/tile";
import { Button } from "./Button";
import { ContextRing } from "./ContextRing";
import { ErrorBoundary } from "./ErrorBoundary";
import { PermissionAnswers } from "./PermissionAnswers";
import { PermissionInput, Revealed } from "./Transcript";
import "./Shelf.css";

interface ShelfProps {
  id?: string;
  /** The session's tile: its title names the shelf (R23), and its readouts are the shelf's (R11). */
  tile: Pick<TileView, "title" | "ring" | "cost">;
  shows: ShelfShows;
  /** Reads the session's events again after reading them failed (R21). */
  onRetry?: () => void;
  /** The session's answers on their way or refused, by request id (R18, R19). */
  answers?: Readonly<Record<string, Answer>>;
  /** Whether answers can reach Core now (R20). */
  live: boolean;
  /** Answers the shown permission request (R13). */
  onDecide: (requestId: string, decision: Decision) => void;
  /** For the gallery: show keyboard focus on the shelf itself without it. */
  force?: "focus";
}

/**
 * The shelf (docs/design/RAIL.md, The shelf): a session's oldest pending request, pulled out of its tile over the
 * transcript's edge. A dialog that leaves the rest of the window usable, read in the order it is shown: what
 * kind, what is asked, its details, the session's readouts, then the answers (R23). `shelfOf` decides what it
 * shows; this only lays it out.
 */
export function Shelf({ id, tile, shows, onRetry, answers, live, onDecide, force }: ShelfProps) {
  const root = useRef<HTMLDivElement>(null);
  const key = shownKey(shows);
  const n = "place" in shows ? (shows.place?.n ?? 1) : 1;
  // Moving to the next request after an answer (its place counts on): the contents cross-fade (docs/design/RAIL.md,
  // The shelf: Motion), and assistive technology hears what is now shown and its place, politely (R24).
  const [last, setLast] = useState({ key, n });
  const [next, setNext] = useState<{ key: string; said: string }>();
  if (key !== last.key) {
    setLast({ key, n });
    if (n > last.n) setNext({ key, said: nextSaid(shows) });
  }
  const moved = next?.key === key;
  useNextHeight(root, key);
  return (
    // Focusable, though not in the tab order, so focus can move to the shelf itself: as it opens (R05), and as an
    // answer makes the answers unavailable.
    <div ref={root} id={id} className="shelf" role="dialog" tabIndex={-1} data-force={force} aria-label={t("rail.shelf.nameLabel", { title: tile.title })} data-tone={shelfTone(shows)}>
      {/* Kept on the page from the start, so what it says once the shelf moves on is heard. */}
      <p className="shelf-said" role="status">
        {next?.said}
      </p>
      {/* Keyed by the request, so the next one's contents fade in afresh. */}
      <div key={key} className="shelf-contents" data-next={moved || undefined}>
        {/* A request it can't draw is said here, in the shelf, and leaves the rail as it was. */}
        <ErrorBoundary what={t("rail.shelf.boundaryName")} resetKey={key}>
          <Contents shows={shows} ring={tile.ring} cost={tile.cost} onRetry={onRetry} answers={answers} live={live} onDecide={onDecide} />
        </ErrorBoundary>
      </div>
    </div>
  );
}

function Contents({
  shows,
  ring,
  cost,
  onRetry,
  answers,
  live,
  onDecide,
}: Pick<ShelfProps, "shows" | "onRetry" | "answers" | "live" | "onDecide"> & { ring?: number; cost?: string }) {
  switch (shows.state) {
    case "loading":
      return (
        <p className="shelf-note" role="status">
          {t("rail.shelf.loadingProgress")}
        </p>
      );
    case "failed":
      return (
        <div className="shelf-failed" role="alert">
          <p className="problem">{t("rail.shelf.readError")}</p>
          <p className="shelf-reason">{shows.message}</p>
          {onRetry && <Button onClick={onRetry}>{t("rail.shelf.retryButton")}</Button>}
        </div>
      );
    case "unknown":
      return (
        <>
          {shows.place && <Heading place={shows.place} />}
          <p className="shelf-note">{t("rail.shelf.unknownNote")}</p>
          <Readouts ring={ring} cost={cost} />
        </>
      );
    case "permission": {
      const { request } = shows;
      const asked = request.description ?? request.display_name ?? request.tool_name;
      return (
        <>
          <Heading kind={t("rail.shelf.permissionHeading")} place={shows.place} />
          <p className="shelf-asked">
            <Revealed text={asked} />
          </p>
          <div className="shelf-details">
            <PermissionInput request={request} />
          </div>
          {request.blocked_path && (
            <p className="shelf-detail">
              {t("rail.shelf.pathLabel")}{" "}
              <span className="mono">
                <Revealed text={request.blocked_path} />
              </span>
            </p>
          )}
          {request.reason && (
            <p className="shelf-detail">
              <Revealed text={request.reason.text} />
            </p>
          )}
          <Readouts ring={ring} cost={cost} />
          <PermissionAnswers request={request} answer={answers?.[request.request_id]} live={live} onDecide={(decision) => onDecide(request.request_id, decision)} />
        </>
      );
    }
    case "question":
      // What a question asks; its header, form and answers come with the question ticket, as the events gain them.
      return (
        <>
          <Heading kind={t("rail.shelf.questionHeading")} place={shows.place} />
          {shows.questions.map((q) => (
            <div key={q.id} className="shelf-question">
              <p className="shelf-question-text">
                <Revealed text={q.text} />
              </p>
            </div>
          ))}
          <Readouts ring={ring} cost={cost} />
        </>
      );
  }
}

/** What assistive technology hears when the shelf moves on: the kind, the place, and what is asked (R24). */
function nextSaid(shows: ShelfShows): string {
  const place = "place" in shows ? shows.place : undefined;
  if (shows.state === "permission") {
    const { request } = shows;
    const asked = request.description ?? request.display_name ?? request.tool_name;
    return t("rail.shelf.nextLabel", { kind: t("rail.shelf.permissionHeading"), n: place?.n ?? 1, m: place?.m ?? 1, asked });
  }
  if (shows.state === "question") {
    const asked = shows.questions[0]?.text ?? "";
    return t("rail.shelf.nextLabel", { kind: t("rail.shelf.questionHeading"), n: place?.n ?? 1, m: place?.m ?? 1, asked });
  }
  return t("rail.shelf.unknownNote");
}

/**
 * As the shelf moves to another request, its height eases from the last one's to the new contents' over
 * `motion-settle` (docs/design/RAIL.md, The shelf: Motion); with reduced motion on, it changes at once.
 */
function useNextHeight(root: { current: HTMLElement | null }, key: string) {
  const height = useRef<number>(undefined);
  const shownKey = useRef(key);
  useLayoutEffect(() => {
    const el = root.current;
    if (!el) return;
    const to = el.getBoundingClientRect().height;
    const from = height.current;
    height.current = to;
    const moved = shownKey.current !== key;
    shownKey.current = key;
    if (!moved || from === undefined || from === to || typeof el.animate !== "function") return;
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;
    // The duration and curve are the motion tokens, read from the page so the tokens stay the one source.
    const style = getComputedStyle(el);
    const duration = milliseconds(style.getPropertyValue("--motion-settle"));
    const easing = style.getPropertyValue("--motion-ease-out").trim() || "ease-out";
    el.animate([{ height: `${from}px` }, { height: `${to}px` }], { duration, easing });
  });
}

/** A CSS time, in milliseconds or seconds, as milliseconds. */
function milliseconds(time: string): number {
  const value = parseFloat(time);
  if (Number.isNaN(value)) return 0;
  return time.trim().endsWith("ms") ? value : value * 1000;
}

/** Which request is shown, so the error boundary tries again once another one is. */
function shownKey(shows: ShelfShows): string {
  if (shows.state === "permission") return `permission:${shows.request.request_id}`;
  if (shows.state === "question") return `question:${shows.requestId}`;
  return shows.state;
}

/** The heading line: the kind of request, and its place at the trailing end when there is more than one. */
function Heading({ kind, place }: { kind?: string; place?: { n: number; m: number } }) {
  return (
    <p className="shelf-heading">
      <span>{kind}</span>
      {place && <span className="shelf-place">{t("rail.shelf.placeLabel", { n: place.n, m: place.m })}</span>}
    </p>
  );
}

/** The context ring with its percentage, a middle dot, and the cost: each only when the tile shows it. */
function Readouts({ ring, cost }: { ring?: number; cost?: string }) {
  if (ring === undefined && cost === undefined) return null;
  return (
    <p className="shelf-readouts">
      {ring !== undefined && <ContextRing used={ring} />}
      {ring !== undefined && cost !== undefined && <span aria-hidden="true">·</span>}
      {cost !== undefined && <span>{cost}</span>}
    </p>
  );
}
