import { Bot, ChevronRight, LoaderCircle } from "lucide-react";
import type { Session } from "../protocol/core-api.gen";
import type { BackendChoices } from "../session/controls";
import { type TileView, tileOf } from "../session/tile";
import { ContextRing } from "./ContextRing";
import { Icon } from "./Icon";
import "./SessionTile.css";

/** Each host's backends, by host id, as `backend.list` gives them. */
export type Backends = Readonly<Record<string, readonly BackendChoices[]>>;

/** For the gallery: show a pointer or keyboard state on one tile without one. */
export type ForcedTile = { id: string; state: "hover" | "focus" | "pressed" };

/**
 * A session's tile (docs/design/RAIL.md, A tile): two lines with no box. Line one is the backend's icon, the title
 * and the state's mark; line two the context ring, under the icon, then a phrase and the cost. `tileOf` decides
 * what each shows; this only lays it out. The mark has the trailing edge to itself, one square on line one.
 */
export function SessionTile({
  session,
  backends,
  active,
  unseen,
  force,
  onSelect,
}: {
  session: Session;
  backends?: Backends;
  active: boolean;
  unseen: boolean;
  force?: ForcedTile["state"];
  onSelect: (id: string) => void;
}) {
  const view = tileOf(session, backends);
  const needsYou = view.mark === "needsYou";
  return (
    // The whole tile is the one control: choosing it anywhere, its chevron included, makes its session active.
    <button type="button" className="session" data-force={force} aria-current={active || undefined} onClick={() => onSelect(session.id)}>
      <span className="session-backend" role="img" aria-label={view.backend}>
        <Icon icon={Bot} size="icon-sm" />
      </span>
      <span className="session-title" data-muted={(view.titleMuted && !unseen) || undefined} data-unseen={unseen || undefined}>
        {view.title}
      </span>
      <span className="session-mark">
        <Mark view={view} />
      </span>
      {view.ring !== undefined && (
        <span className="session-ring">
          <ContextRing used={view.ring} size="ring-sm" />
        </span>
      )}
      <span className="session-detail">
        {/* A want is named by its chevron, so it isn't read twice. */}
        {view.phrase && (
          <span className="session-phrase" data-tone={needsYou ? view.tone : undefined} aria-hidden={needsYou || undefined}>
            {view.phrase}
          </span>
        )}
        {view.phrase && view.cost && " · "}
        {view.cost}
      </span>
    </button>
  );
}

/** The state's mark, named where line two doesn't already name it. */
function Mark({ view }: { view: TileView }) {
  switch (view.mark) {
    case "starting":
      return (
        <span className="session-arc" aria-hidden="true">
          <Icon icon={LoaderCircle} size="icon-xs" />
        </span>
      );
    case "working":
      return <span className="glyph session-glyph" data-tone={view.tone} role="img" aria-label={view.markLabel} />;
    case "needsYou":
      // A mark, not a control, until the shelf is built (LAY-SHELL, Built so far).
      return (
        <span className="session-chevron" data-tone={view.tone} role="img" aria-label={view.markLabel}>
          <Icon icon={ChevronRight} size="icon-sm" />
        </span>
      );
    // Line two names these, so their marks are hidden from assistive technology.
    case "disconnected":
    case "error":
      return <span className="glyph session-glyph" data-tone={view.tone} aria-hidden="true" />;
    default:
      return null;
  }
}
