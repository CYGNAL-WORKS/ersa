import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { t } from "../catalog";
import type { Answer, Decision } from "../session/answers";
import type { PermissionRequested } from "../protocol/agent-events.gen";
import { type ShelfShows, shelfTone, shownRequest } from "../session/shelf";
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
  const place = "place" in shows ? shows.place : undefined;
  // Moving from one request to the next, its place counting on, is moving on after an answer (R10): the contents
  // cross-fade (docs/design/RAIL.md, The shelf: Motion), and assistive technology hears what is now shown and its
  // place, politely (R24). Coming back to a request after loading, as on a reconnect, is not moving on.
  const [last, setLast] = useState({ key, shows });
  const [next, setNext] = useState<{ key?: string; said: string; from?: ShelfShows }>();
  if (key !== last.key) {
    setLast({ key, shows });
    const lastPlace = "place" in last.shows ? last.shows.place : undefined;
    if (shownRequest(last.shows) !== undefined && (place?.n ?? 1) > (lastPlace?.n ?? 1)) {
      // The last request's contents fade out over the new ones, unless reduced motion is on.
      setNext({ key, said: nextSaid(shows), from: reducedMotion() ? undefined : last.shows });
    } else {
      // Not moving on: nothing fades, and what was last said stays said.
      setNext((n) => n && { said: n.said });
    }
  }
  const moved = next?.key === key;
  const leaving = moved ? next.from : undefined;
  useMovingOn(root, key, moved);
  const contents = (shown: ShelfShows) => (
    // A request it can't draw is said here, in the shelf, and leaves the rail as it was.
    <ErrorBoundary what={t("rail.shelf.boundaryName")} resetKey={shownKey(shown)}>
      <Contents shows={shown} ring={tile.ring} cost={tile.cost} onRetry={onRetry} answers={answers} live={live} onDecide={onDecide} />
    </ErrorBoundary>
  );
  return (
    // Focusable, though not in the tab order, so focus can move to the shelf itself: as it opens (R05), as an answer
    // makes the answers unavailable, and as it moves on (R10).
    <div ref={root} id={id} className="shelf" role="dialog" tabIndex={-1} data-force={force} aria-label={t("rail.shelf.nameLabel", { title: tile.title })} data-tone={shelfTone(shows)}>
      {/* Kept on the page from the start, so what it says once the shelf moves on is heard. */}
      <p className="shelf-said" role="status">
        {next?.said}
      </p>
      {/* Keyed by the request, so the next one's contents fade in afresh. */}
      <div key={key} className="shelf-contents" data-next={moved || undefined}>
        {contents(shows)}
      </div>
      {/* The last request's contents, fading out over the new ones; out of reach and unheard meanwhile. */}
      {leaving && (
        <div
          key={`leaving:${shownKey(leaving)}`}
          className="shelf-contents"
          data-leaving
          inert
          aria-hidden="true"
          onAnimationEnd={() => setNext((n) => n && { ...n, from: undefined })}
        >
          {contents(leaving)}
        </div>
      )}
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
      const asked = askedOf(request);
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

/** What a permission request asks: its description, else the tool's display name, else its name (R11). */
function askedOf(request: PermissionRequested): string {
  return request.description ?? request.display_name ?? request.tool_name;
}

/** What assistive technology hears when the shelf moves on: the kind, the place, and what is asked (R24). */
function nextSaid(shows: ShelfShows): string {
  const place = "place" in shows ? shows.place : undefined;
  const at = { n: place?.n ?? 1, m: place?.m ?? 1 };
  if (shows.state === "permission") return t("rail.shelf.nextLabel", { kind: t("rail.shelf.permissionHeading"), ...at, asked: askedOf(shows.request) });
  if (shows.state === "question") return t("rail.shelf.nextLabel", { kind: t("rail.shelf.questionHeading"), ...at, asked: shows.questions[0]?.text ?? "" });
  return t("rail.shelf.nextUnknownLabel", { ...at, note: t("rail.shelf.unknownNote") });
}

function reducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * As the shelf moves on to another request: focus that was in the shelf goes to the shelf itself, since the
 * answer that held it has gone with the last request (R10); and its height eases from the last request's to the
 * new contents' over `motion-settle` (docs/design/RAIL.md, The shelf: Motion), or changes at once with reduced
 * motion on.
 */
function useMovingOn(root: { current: HTMLElement | null }, key: string, moved: boolean) {
  const height = useRef<number>(undefined);
  const lastKey = useRef(key);
  const easing = useRef<Animation>(undefined);
  const focused = useRef(false);
  useEffect(() => {
    const el = root.current;
    if (!el) return;
    // Focus that goes nowhere (the element holding it is gone) leaves this as it was.
    const focusIn = () => {
      focused.current = true;
    };
    const focusOut = (event: FocusEvent) => {
      if (event.relatedTarget !== null) focused.current = el.contains(event.relatedTarget as Node);
    };
    el.addEventListener("focusin", focusIn);
    el.addEventListener("focusout", focusOut);
    return () => {
      el.removeEventListener("focusin", focusIn);
      el.removeEventListener("focusout", focusOut);
    };
  }, [root]);
  useLayoutEffect(() => {
    const el = root.current;
    if (!el) return;
    const changed = lastKey.current !== key;
    lastKey.current = key;
    if (changed && moved && focused.current && !el.contains(document.activeElement)) el.focus();
    // An ease still running starts the next from where it has got to, and the new height is measured without it.
    const from = easing.current ? el.getBoundingClientRect().height : height.current;
    easing.current?.cancel();
    easing.current = undefined;
    const to = el.getBoundingClientRect().height;
    height.current = to;
    if (!changed || !moved || from === undefined || from === to || typeof el.animate !== "function" || reducedMotion()) return;
    // The duration and curve are the motion tokens, read from the page so the tokens stay the one source.
    const style = getComputedStyle(el);
    const duration = milliseconds(style.getPropertyValue("--motion-settle"));
    const curve = style.getPropertyValue("--motion-ease-out").trim();
    if (!duration || !curve) return;
    const animation = el.animate([{ height: `${from}px` }, { height: `${to}px` }], { duration, easing: curve });
    easing.current = animation;
    animation.onfinish = () => {
      if (easing.current === animation) easing.current = undefined;
    };
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
  const request = shownRequest(shows);
  return request === undefined ? shows.state : `${shows.state}:${request}`;
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
