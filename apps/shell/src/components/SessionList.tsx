import type { SessionsView } from "../core/connection";
import type { Session } from "../protocol/core-api.gen";
import { stateText } from "../session/states";
import "./SessionList.css";

interface SessionListProps {
  view: SessionsView;
  activeId?: string;
  onSelect: (id: string) => void;
  force?: ForcedTile;
  /** Each workspace's name, by id: given, the tiles sit under their workspace (LAY-SHELL); else one list. */
  workspaces?: Readonly<Record<string, WorkspaceName>>;
}

/** What the rail calls a workspace: its project, and the branch or "Root workspace". */
export interface WorkspaceName {
  project: string;
  name: string;
}

/** The sessions Core runs, as tiles: newest first, the active one outlined, one that needs the user marked with a glyph. */
export function SessionList({ view, activeId, onSelect, force, workspaces }: SessionListProps) {
  const tile = (s: Session) => (
    <li key={s.id}>
      <SessionTile session={s} active={s.id === activeId} force={force?.id === s.id ? force.state : undefined} onSelect={onSelect} />
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
              {name && (
                <h3 className="sessions-workspace-name">
                  <span className="sessions-workspace-project">{name.project}</span> <span>{name.name}</span>
                </h3>
              )}
              <ul className="sessions-list">{sessions.map(tile)}</ul>
            </section>
          );
        })}
    </section>
  );
}

/** For the gallery: show a pointer or keyboard state on one tile without one. */
export type ForcedTile = { id: string; state: "hover" | "focus" | "pressed" };

function SessionTile({
  session,
  active,
  force,
  onSelect,
}: {
  session: Session;
  active: boolean;
  force?: ForcedTile["state"];
  onSelect: (id: string) => void;
}) {
  const { tile } = session;
  const needsYou = tile.pending.length > 0;
  return (
    <button
      type="button"
      className="session"
      data-needs-you={needsYou || undefined}
      data-force={force}
      aria-current={active || undefined}
      onClick={() => onSelect(session.id)}
    >
      <span className="glyph session-glyph" data-tone={needsYou ? "attention" : toneOf[tile.state]} aria-hidden="true" />
      <span className="session-title">{session.title ?? "Untitled session"}</span>
      <span className="session-state">
        {needsYou ? (
          <>
            Needs you · <span className="readout">{tile.pending.length}</span>
          </>
        ) : (
          stateText(tile.state)
        )}
      </span>
      {tile.cost_usd_total !== undefined && <span className="session-cost readout">${tile.cost_usd_total.toFixed(2)}</span>}
    </button>
  );
}

/** The glyph's tone for each lifecycle state; a pending request overrides it with attention. */
const toneOf: Record<Session["tile"]["state"], "positive" | "pending" | "danger" | undefined> = {
  starting: "pending",
  running: "positive",
  waiting_user: undefined,
  idle: undefined,
  disconnected: "danger",
  ended: undefined,
  error: "danger",
};

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
