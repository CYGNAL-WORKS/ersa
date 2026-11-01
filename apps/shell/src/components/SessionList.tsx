import type { SessionsView } from "../core/connection";
import type { Session } from "../protocol/core-api.gen";
import { stateText } from "../session/states";
import "./SessionList.css";

interface SessionListProps {
  view: SessionsView;
  activeId?: string;
  onSelect: (id: string) => void;
}

/** The sessions Core runs, as tiles: newest first, the active one outlined, one that needs the user marked with a glyph. */
export function SessionList({ view, activeId, onSelect }: SessionListProps) {
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
      {view.state === "ready" && view.sessions.length > 0 && (
        <ul className="sessions-list">
          {newestFirst(view.sessions).map((s) => (
            <li key={s.id}>
              <SessionTile session={s} active={s.id === activeId} onSelect={onSelect} />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function SessionTile({ session, active, onSelect }: { session: Session; active: boolean; onSelect: (id: string) => void }) {
  const { tile } = session;
  const needsYou = tile.pending.length > 0;
  return (
    <button
      type="button"
      className="session"
      data-state={tile.state}
      data-needs-you={needsYou || undefined}
      aria-current={active || undefined}
      onClick={() => onSelect(session.id)}
    >
      <span className="session-glyph" aria-hidden="true" />
      <span className="session-title">{session.title ?? "Untitled session"}</span>
      <span className="session-state">{needsYou ? `Needs you (${tile.pending.length})` : stateText(tile.state)}</span>
      {tile.cost_usd_total !== undefined && <span className="session-cost readout">${tile.cost_usd_total.toFixed(2)}</span>}
    </button>
  );
}

/** Core issues `created_at` as RFC 3339 in UTC, so the strings sort as times do. */
export function newestFirst(sessions: Session[]): Session[] {
  return sessions.slice().sort((a, b) => (a.created_at < b.created_at ? 1 : a.created_at > b.created_at ? -1 : 0));
}
