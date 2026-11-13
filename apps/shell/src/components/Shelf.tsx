import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { t } from "../catalog";
import type { Answer, Decision, Reply } from "../session/answers";
import type { PermissionRequested } from "../protocol/agent-events.gen";
import { type ShelfShows, shelfTone, shownRequest } from "../session/shelf";
import type { TileView } from "../session/tile";
import { Button } from "./Button";
import { ContextRing } from "./ContextRing";
import { HeightEase, reducedMotion } from "../theme/motion";
import { ErrorBoundary } from "./ErrorBoundary";
import { PermissionAnswers } from "./PermissionAnswers";
import { QuestionForm } from "./QuestionForm";
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
  /** Answers the shown question, or declines it (R16, R28). */
  onAnswer: (requestId: string, reply: Reply) => void;
  /** For the gallery: show keyboard focus on the shelf itself without it. */
  force?: "focus";
}

/**
 * The shelf (docs/design/RAIL.md, The shelf): a session's oldest pending request, pulled out of its tile over the
 * transcript's edge. A dialog that leaves the rest of the window usable, read in the order it is shown: what
 * kind, what is asked, its details, the session's readouts, then the answers (R23). `shelfOf` decides what it
 * shows; this only lays it out.
 */
export function Shelf({ id, tile, shows, onRetry, answers, live, onDecide, onAnswer, force }: ShelfProps) {
  const root = useRef<HTMLDivElement>(null);
  const key = shownKey(shows);
  // A request, or one it can't show: what the shelf counts its place by.
  const request = shows.state === "permission" || shows.state === "question" || shows.state === "unknown" ? shows : undefined;
  // Moving to a request whose place counts on from the last one shown, whatever came between (loading, or the last
  // request settled), is moving on after an answer (R10): the contents cross-fade (docs/design/RAIL.md, The shelf:
  // Motion), and assistive technology hears what is now shown and its place, politely (R24). Coming back to the
  // same place after loading, as on a reconnect, is not moving on.
  const [last, setLast] = useState<{ key: string; shown?: ShelfShows; onScreen: boolean }>({ key, shown: request, onScreen: request !== undefined });
  const [next, setNext] = useState<{ key?: string; said: string; from?: ShelfShows }>();
  if (key !== last.key) {
    // The last request stays on screen while the shelf settles.
    setLast({ key, shown: request ?? last.shown, onScreen: request !== undefined || shows.state === "settled" });
    if (request && last.shown && placeOf(request) > placeOf(last.shown)) {
      // What was on screen fades out over the new contents, unless reduced motion is on.
      setNext({ key, said: nextSaid(request), from: reducedMotion() || !last.onScreen ? undefined : last.shown });
    } else {
      // Not moving on: nothing fades, and what was last said stays said.
      setNext((n) => n && { said: n.said });
    }
  }
  const moved = next?.key === key;
  const leaving = moved ? next.from : undefined;
  // Settled: the last request, its answers unavailable, until the shelf closes; never a "Loading…" nothing is doing.
  const settled = shows.state === "settled";
  const display = settled ? last.shown : shows;
  useMovingOn(root, key, moved);
  const contents = (shown: ShelfShows, done = false) => (
    // A request it can't draw is said here, in the shelf, and leaves the rail as it was.
    <ErrorBoundary what={t("rail.shelf.boundaryName")} resetKey={shownKey(shown)}>
      <Contents shows={shown} ring={tile.ring} cost={tile.cost} onRetry={onRetry} answers={answers} live={live} settled={done} onDecide={onDecide} onAnswer={onAnswer} />
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
        {display && contents(display, settled)}
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
          {contents(leaving, true)}
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
  settled,
  onDecide,
  onAnswer,
}: Pick<ShelfProps, "shows" | "onRetry" | "answers" | "live" | "onDecide" | "onAnswer"> & { ring?: number; cost?: string; settled: boolean }) {
  switch (shows.state) {
    case "settled":
      return null;
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
          <PermissionAnswers
            request={request}
            answer={answers?.[request.request_id]}
            // Answered already, as the shelf settles or as it fades out: nothing to choose, and nothing to reconnect for.
            live={live && !settled}
            reconnectingNote={!settled}
            onDecide={(decision) => onDecide(request.request_id, decision)}
          />
        </>
      );
    }
    case "question":
      // Its questions and their form, then the session's readouts, then Decline and Answer (R15, R16, R23, R28).
      return (
        <>
          <Heading kind={t("rail.shelf.questionHeading")} place={shows.place} />
          <QuestionForm
            questions={shows.questions}
            answer={answers?.[shows.requestId]}
            // Answered already, as the shelf settles or as it fades out: nothing to choose, and nothing to reconnect for.
            live={live && !settled}
            reconnectingNote={!settled}
            between={<Readouts ring={ring} cost={cost} />}
            onAnswer={(reply) => onAnswer(shows.requestId, reply)}
          />
        </>
      );
  }
}

/** A request's place, n, for telling whether the shelf moved on. */
function placeOf(shows: ShelfShows): number {
  return ("place" in shows ? shows.place?.n : undefined) ?? 1;
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

/**
 * As the shelf moves on to another request: focus that was in the shelf goes to the shelf itself, since the
 * answer that held it has gone with the last request (R10); and its height eases from the last request's to the
 * new contents' over `motion-settle` (docs/design/RAIL.md, The shelf: Motion), or changes at once with reduced
 * motion on.
 */
function useMovingOn(root: { current: HTMLElement | null }, key: string, moved: boolean) {
  const ease = useRef(new HeightEase()).current;
  const lastKey = useRef(key);
  const focused = useRef(false);
  useEffect(() => {
    const el = root.current;
    if (!el) return;
    // An element removed while it holds focus sends no focusout, so the answer that goes with the last request
    // leaves this true. Focus the user moves elsewhere, or out of the window, does send one; coming back sends a
    // focusin.
    const focusIn = () => {
      focused.current = true;
    };
    const focusOut = (event: FocusEvent) => {
      focused.current = event.relatedTarget instanceof Node && el.contains(event.relatedTarget);
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
    if (!changed) {
      // Another render of the same request leaves a running ease alone.
      ease.hold(el);
      return;
    }
    if (moved && focused.current && !el.contains(document.activeElement)) el.focus();
    ease.change(el, moved);
  });
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
