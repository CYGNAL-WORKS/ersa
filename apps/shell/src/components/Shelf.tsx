import { t } from "../catalog";
import type { ShelfShows } from "../session/shelf";
import { Button } from "./Button";
import { ContextRing } from "./ContextRing";
import { PermissionInput, Revealed } from "./Transcript";
import "./Shelf.css";

interface ShelfProps {
  id?: string;
  /** The session's title, which names the shelf (FLOW-APPROVE-PERMISSION-R23). */
  title: string;
  shows: ShelfShows;
  /** The session's readouts as its tile shows them: the context ring's share used, and the cost (R11). */
  ring?: number;
  cost?: string;
  /** Reads the session's events again after reading them failed (R21). */
  onRetry?: () => void;
}

/**
 * The shelf (docs/design/RAIL.md, The shelf): a session's oldest pending request, pulled out of its tile over the
 * transcript's edge. A dialog that leaves the rest of the window usable, read in the order it is shown: what
 * kind, what is asked, its details, the session's readouts, then the answers (R23). `shelfOf` decides what it
 * shows; this only lays it out. The answers are drawn here and wired by the answers ticket.
 */
export function Shelf({ id, title, shows, ring, cost, onRetry }: ShelfProps) {
  return (
    <div id={id} className="shelf" role="dialog" aria-label={t("rail.shelf.nameLabel", { title })} data-tone={shows.state === "question" ? "accent" : "caution"}>
      <Contents shows={shows} ring={ring} cost={cost} onRetry={onRetry} />
    </div>
  );
}

function Contents({ shows, ring, cost, onRetry }: Omit<ShelfProps, "title" | "id">) {
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
      // Deny leads when the request says it defaults to no; Allow otherwise.
      const denyFirst = request.flags.default_to_no;
      const suggestions = request.flags.suppress_always_allow ? [] : request.suggestions;
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
          <div className="shelf-answers">
            {suggestions.map((s, i) => (
              <Button key={i} className="shelf-suggestion">
                <Revealed text={s.label} />
              </Button>
            ))}
            <div className="shelf-answer-row">
              <Button variant={denyFirst ? "primary" : "quiet"}>{t("rail.shelf.denyButton")}</Button>
              <Button variant={denyFirst ? "quiet" : "primary"}>{t("rail.shelf.allowButton")}</Button>
            </div>
          </div>
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

/** The heading line: the kind of request, and its place at the trailing end when there is more than one. */
function Heading({ kind, place }: { kind?: string; place?: { n: number; m: number } }) {
  return (
    <p className="shelf-heading">
      <span>{kind}</span>
      {place && <span className="readout">{t("rail.shelf.placeLabel", { n: place.n, m: place.m })}</span>}
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
