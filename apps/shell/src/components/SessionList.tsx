import { Bot, ChevronRight, LoaderCircle } from "lucide-react";
import { type AnimationEvent, type CSSProperties, type ReactNode, useId, useLayoutEffect, useRef, useState } from "react";
import { t } from "../catalog";
import type { SessionsView } from "../core/connection";
import type { Session } from "../protocol/core-api.gen";
import { type BackendChoices, newestFirst } from "../session/controls";
import { type TileView, tileOf } from "../session/tile";
import { ContextRing } from "./ContextRing";
import { Icon } from "./Icon";
import "./SessionList.css";

/** Each host's backends, by host id, as `backend.list` gives them. */
type Backends = Readonly<Record<string, readonly BackendChoices[]>>;

interface SessionListProps {
  view: SessionsView;
  activeId?: string;
  onSelect: (id: string) => void;
  force?: ForcedTile;
  /** Each workspace's name, by id: given, the tiles sit under their workspace (LAY-SHELL); else one list. */
  workspaces?: Readonly<Record<string, WorkspaceName>>;
  /** Each host's backends, to name a tile's backend by its own name; until they are read, it is named by its id. */
  backends?: Backends;
  /** The unseen sessions' ids (LAY-SHELL, Unseen): their titles read strong until they are made active. */
  unseen?: ReadonlySet<string>;
  /** The open shelf: its session, its tone, and the shelf itself, which is drawn beside that session's tile. */
  shelf?: OpenShelf;
  /** The user chose a tile's chevron (FLOW-APPROVE-PERMISSION-R01). */
  onChevron?: (id: string) => void;
}

/** The open shelf, as the rail draws it beside its tile. `render` is given the element id the chevron controls. */
export interface OpenShelf {
  id: string;
  tone: "accent" | "caution";
  render: (domId: string) => ReactNode;
}

/** What the rail calls a workspace: its project, and the branch or "Root workspace". */
export interface WorkspaceName {
  project: string;
  name: string;
  /** The name is a path, and is set as one. */
  path?: boolean;
}

/** The sessions Core runs, as tiles: newest first, the active one quietly highlighted. */
export function SessionList({ view, activeId, onSelect, force, workspaces, backends, unseen, shelf, onChevron }: SessionListProps) {
  const tile = (s: Session) => {
    const open = shelf?.id === s.id ? shelf : undefined;
    return (
      <TileItem key={s.id} shelf={open}>
        {(shelfDomId) => (
          <SessionTile
            session={s}
            backends={backends}
            active={s.id === activeId}
            unseen={unseen?.has(s.id) ?? false}
            force={force?.id === s.id ? force.state : undefined}
            onSelect={onSelect}
            shelf={open && { tone: open.tone, domId: shelfDomId }}
            onChevron={onChevron}
          />
        )}
      </TileItem>
    );
  };
  return (
    <section className="sessions" aria-label="Sessions">
      <h2 className="sessions-header">
        <span>Sessions</span>
        {view.state === "ready" && <span className="readout">{view.sessions.length}</span>}
      </h2>
      {view.state === "loading" && <p className="sessions-note">Loading…</p>}
      {view.state === "unavailable" && <p className="sessions-note">Sessions appear once Core is connected.</p>}
      {view.state === "error" && (
        <p className="problem" role="alert">
          {view.message}
        </p>
      )}
      {view.state === "ready" && view.sessions.length === 0 && (
        <p className="sessions-note">
          No sessions yet.
          <span className="sessions-note-hint">Start one from a project above.</span>
        </p>
      )}
      {view.state === "ready" && view.sessions.length > 0 && !workspaces && <ul className="sessions-list">{newestFirst(view.sessions).map(tile)}</ul>}
      {view.state === "ready" &&
        view.sessions.length > 0 &&
        workspaces &&
        byWorkspace(view.sessions).map(([id, sessions]) => {
          const name = workspaces[id];
          return (
            <section key={id} className="sessions-workspace" aria-label={name ? `${name.project}, ${name.name}` : "Workspace"}>
              {/* Its line is kept while the name is still being read, so the tiles never move when it arrives. */}
              <h3 className="sessions-workspace-name" aria-hidden={name ? undefined : true}>
                {name ? (
                  <>
                    <span className="sessions-workspace-project">{name.project}</span>{" "}
                    <span className={name.path ? "mono" : undefined}>{name.name}</span>
                  </>
                ) : (
                  " "
                )}
              </h3>
              <ul className="sessions-list">{sessions.map(tile)}</ul>
            </section>
          );
        })}
    </section>
  );
}

/** For the gallery: show a pointer or keyboard state on one tile without one. */
export type ForcedTile = { id: string; state: "hover" | "focus" | "pressed" };

/**
 * A session's tile (docs/design/RAIL.md, A tile): two lines with no box. Line one is the backend's icon, the title
 * and the state's mark; line two the context ring, under the icon, then a phrase and the cost. `tileOf` decides
 * what each shows; this only lays it out. The mark has the trailing edge to itself, one square on line one.
 */
function SessionTile({
  session,
  backends,
  active,
  unseen,
  force,
  onSelect,
  shelf,
  onChevron,
}: {
  session: Session;
  backends?: Backends;
  active: boolean;
  unseen: boolean;
  force?: ForcedTile["state"];
  onSelect: (id: string) => void;
  /** Its shelf is open: the shelf's tone, which the chevron and the want take, and the shelf's element id. */
  shelf?: { tone: "accent" | "caution"; domId: string };
  onChevron?: (id: string) => void;
}) {
  const view = tileOf(session, backends);
  const needsYou = view.mark === "needsYou";
  // The chevron is the shelf's handle on every tile but the active one, whose requests the transcript answers
  // (FLOW-APPROVE-PERMISSION-R26); there it stays a mark that names the want.
  const handle = needsYou && !active;
  const tone = shelf?.tone ?? view.tone;
  return (
    <>
      {/* The tile makes its session active; the chevron, laid over its trailing square, opens its shelf. */}
      <button type="button" className="session" data-force={force} aria-current={active || undefined} onClick={() => onSelect(session.id)}>
        <span className="session-backend" role="img" aria-label={view.backend}>
          <Icon icon={Bot} size="icon-sm" />
        </span>
        <span className="session-title" data-muted={(view.titleMuted && !unseen) || undefined} data-unseen={unseen || undefined}>
          {view.title}
        </span>
        <span className="session-mark">{!handle && <Mark view={view} />}</span>
        {view.ring !== undefined && (
          <span className="session-ring">
            <ContextRing used={view.ring} size="ring-sm" />
          </span>
        )}
        <span className="session-detail">
          {/* On the active tile a want is named by its chevron mark, so it isn't read twice. */}
          {view.phrase && (
            <span className="session-phrase" data-tone={needsYou ? tone : undefined} aria-hidden={(needsYou && !handle) || undefined}>
              {view.phrase}
            </span>
          )}
          {view.phrase && view.cost && " · "}
          {view.cost}
        </span>
      </button>
      {handle && (
        <button
          type="button"
          className="session-chevron-button"
          data-tone={tone}
          aria-label={t("rail.shelf.nameLabel", { title: view.title })}
          aria-haspopup="dialog"
          aria-expanded={shelf !== undefined}
          aria-controls={shelf?.domId}
          onClick={() => onChevron?.(session.id)}
        >
          <Icon icon={ChevronRight} size="icon-sm" />
        </button>
      )}
    </>
  );
}

/**
 * A tile's place in the list and, while its shelf is open, the shelf beside it. The open tile names the anchor the
 * shelf is placed by, and tells the shelf where the two join, so they read as one shape (docs/design/RAIL.md, The
 * shelf: One shape).
 */
function TileItem({ shelf, children }: { shelf?: OpenShelf; children: (shelfDomId: string) => ReactNode }) {
  const item = useRef<HTMLLIElement>(null);
  const domId = useId();
  const open = shelf !== undefined;
  const shown = shelf?.render(domId);
  // The shelf last drawn, kept while it slides back after closing (docs/design/RAIL.md, The shelf: Motion).
  const last = useRef<ReactNode>(undefined);
  // Kept once committed, so a render React discards never slides back.
  useLayoutEffect(() => {
    if (shown !== undefined) last.current = shown;
  });
  // Decided in the render that closes it, so the layer never leaves the page for a frame and the shelf keeps its
  // element, and the join measured on it, as it slides back.
  const [closing, setClosing] = useState<ReactNode>();
  const [wasOpen, setWasOpen] = useState(open);
  if (open !== wasOpen) {
    setWasOpen(open);
    // With reduced motion on, it goes at once.
    const still = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    setClosing(open || still ? undefined : last.current);
  }
  const closed = (event: AnimationEvent) => {
    if (event.animationName === "shelf-back") setClosing(undefined);
  };
  useLayoutEffect(() => {
    const li = item.current;
    const layer = li?.querySelector<HTMLElement>(".shelf");
    if (!li || !layer) return;
    // Measured, not designed: where the tile meets the shelf, which moves as the rail scrolls or the shelf rises.
    const place = () => {
      const tile = li.getBoundingClientRect();
      const box = layer.getBoundingClientRect();
      const top = tile.top - box.top;
      layer.style.setProperty("--shelf-join-top", `${top}px`);
      layer.style.setProperty("--shelf-join-height", `${tile.height}px`);
      layer.toggleAttribute("data-risen", top > 0.5);
    };
    place();
    const observer = new ResizeObserver(place);
    observer.observe(li);
    observer.observe(layer);
    window.addEventListener("resize", place);
    document.addEventListener("scroll", place, true);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", place);
      document.removeEventListener("scroll", place, true);
    };
  }, [open]);
  const layer = shown ?? closing;
  // Each tile names its own anchor, so a shelf sliding back stays by its tile while another tile's opens.
  const anchor = `--tile-${domId.replace(/[^\w-]/g, "")}`;
  return (
    <li
      ref={item}
      className="session-item"
      data-shelf={open || undefined}
      data-tone={shelf?.tone}
      style={layer !== undefined ? ({ anchorName: anchor } as CSSProperties) : undefined}
    >
      {children(domId)}
      {/* One layer, open or closing, so the shelf keeps its element, and its measured join, as it slides back. */}
      {layer !== undefined && (
        <div className="shelf-layer" inert={!open || undefined} onAnimationEnd={closed} style={{ positionAnchor: anchor } as CSSProperties}>
          {layer}
        </div>
      )}
    </li>
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
      // The active tile's: a mark, not a control, since the transcript answers its requests.
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

/** The sessions by workspace, newest first within each, and the workspace with the newest session first. */
export function byWorkspace(sessions: Session[]): [string, Session[]][] {
  const groups = new Map<string, Session[]>();
  for (const s of newestFirst(sessions)) groups.set(s.workspace_id, [...(groups.get(s.workspace_id) ?? []), s]);
  return [...groups];
}

