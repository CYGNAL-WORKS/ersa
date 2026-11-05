import { Bot, ChevronRight, LoaderCircle } from "lucide-react";
import type { SessionsView } from "../core/connection";
import type { Session } from "../protocol/core-api.gen";
import type { BackendChoices } from "../session/controls";
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
}

/** What the rail calls a workspace: its project, and the branch or "Root workspace". */
export interface WorkspaceName {
  project: string;
  name: string;
  /** The name is a path, and is set as one. */
  path?: boolean;
}

/** The sessions Core runs, as tiles: newest first, the active one quietly highlighted. */
export function SessionList({ view, activeId, onSelect, force, workspaces, backends }: SessionListProps) {
  const tile = (s: Session) => (
    <li key={s.id}>
      <SessionTile session={s} backends={backends} active={s.id === activeId} force={force?.id === s.id ? force.state : undefined} onSelect={onSelect} />
    </li>
  );
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
  force,
  onSelect,
}: {
  session: Session;
  backends?: Backends;
  active: boolean;
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
      <span className="session-title" data-muted={view.titleMuted || undefined}>
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

/** The sessions by workspace, newest first within each, and the workspace with the newest session first. */
export function byWorkspace(sessions: Session[]): [string, Session[]][] {
  const groups = new Map<string, Session[]>();
  for (const s of newestFirst(sessions)) groups.set(s.workspace_id, [...(groups.get(s.workspace_id) ?? []), s]);
  return [...groups];
}

/** Core issues `created_at` as RFC 3339 in UTC, so the strings sort as times do. */
export function newestFirst(sessions: Session[]): Session[] {
  return sessions.slice().sort((a, b) => (a.created_at < b.created_at ? 1 : a.created_at > b.created_at ? -1 : 0));
}
