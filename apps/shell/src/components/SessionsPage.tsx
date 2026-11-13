import { Plus, Search } from "lucide-react";
import { type KeyboardEvent, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { t } from "../catalog";
import type { SessionsView } from "../core/connection";
import type { Project, Session, Workspace } from "../protocol/core-api.gen";
import { type SessionsTab, lastActive, pageRows } from "../session/sessionsPage";
import { tileOf } from "../session/tile";
import { Button } from "./Button";
import { ErrorBoundary } from "./ErrorBoundary";
import { Icon, IconButton } from "./Icon";
import { Problem, Refusals } from "./RailTree";
import { type Backends, type ForcedTile, Mark } from "./SessionTile";
import { SessionRoot, type TileActions, type TileHandlers, tileActions, useTileMenu } from "./TileMenu";
import { useLeavingFocus } from "./useLeavingFocus";
import "./SessionsPage.css";

const TABS = [
  { tab: "active", name: "rail.sessionsPage.activeTabButton" },
  { tab: "archived", name: "rail.sessionsPage.archivedTabButton" },
] as const satisfies readonly { tab: SessionsTab; name: string }[];

/** Now, moved on each minute while the page shows, so "5 minutes ago" keeps true. */
function useMinute(): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const ticking = setInterval(() => setNow(new Date()), 60_000);
    return () => clearInterval(ticking);
  }, []);
  return now;
}

/**
 * The sessions page (FLOW-ORGANIZE-SESSIONS, step 9; docs/design/RAIL.md, The sessions page): every session in one
 * list, whatever the rail's arrangement, with Active and Archived tabs and a search. A row's menu is the tile's menu.
 */
export function SessionsPage({
  sessions,
  projects,
  workspaces,
  localHost,
  hostNames,
  backends,
  onOpen,
  onNewSession,
  now: fixedNow,
  initialTab = "active",
  initialSearch,
  force,
  forceTab,
  forceSearch,
  initialTileMenu,
  initialTileDraft,
  ...handlers
}: TileHandlers & {
  sessions: SessionsView;
  /** The projects as last read, to name each row's; a session whose project is not known yet names none. */
  projects?: readonly Project[];
  workspaces: Readonly<Record<string, Pick<Workspace, "project_id">>>;
  /** The local machine's host id: a session on any other host is named with its host. */
  localHost?: string;
  /** Each host's name, by id; a host not listed is named by its id. */
  hostNames?: Readonly<Record<string, string>>;
  backends?: Backends;
  /** Opens a session: it becomes the active session and the page gives way (R18). */
  onOpen: (id: string) => void;
  onNewSession: () => void;
  /** For the gallery: a fixed now, a chosen tab, the search open with this text, a row's state, its menu, its title being edited. */
  now?: Date;
  initialTab?: SessionsTab;
  initialSearch?: string;
  force?: ForcedTile;
  forceTab?: { tab: SessionsTab; state: "hover" | "focus" | "pressed" };
  forceSearch?: boolean;
  initialTileMenu?: string;
  initialTileDraft?: { id: string; draft: string };
}) {
  const minute = useMinute();
  const now = fixedNow ?? minute;
  const [tab, setTab] = useState<SessionsTab>(initialTab);
  // The search text, while the field is open.
  const [search, setSearch] = useState(initialSearch);
  const searching = search !== undefined;
  const tabsId = useId();

  const page = useRef<HTMLElement>(null);
  // New session is unavailable while Ersa can't reach Core, when every row goes at once: the chosen tab takes focus then.
  const leaving = useLeavingFocus(page, (root) => root.querySelector<HTMLElement>(".sessions-page-new:not(:disabled)") ?? root.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]'));

  // The field takes focus as the user opens it, and the search button as Escape closes it. Moved after the render
  // that swaps one for the other.
  const field = useRef<HTMLInputElement>(null);
  const searchButton = useRef<HTMLButtonElement>(null);
  const focusNext = useRef<"field" | "button">(undefined);
  useLayoutEffect(() => {
    if (focusNext.current === "field") field.current?.focus();
    if (focusNext.current === "button") searchButton.current?.focus();
    focusNext.current = undefined;
  }, [searching]);

  const tabButtons = useRef<Partial<Record<SessionsTab, HTMLButtonElement | null>>>({});
  const tabKey = (event: KeyboardEvent) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const other = tab === "active" ? "archived" : "active";
    setTab(other);
    tabButtons.current[other]?.focus();
  };

  const projectById = new Map(projects?.map((p) => [p.id, p]));
  const hostOf = (s: Session) => (localHost !== undefined && s.host_id !== localHost ? (hostNames?.[s.host_id] ?? s.host_id) : undefined);
  const projectOf = (s: Session) => {
    const workspace = workspaces[s.workspace_id];
    return workspace && projectById.get(workspace.project_id)?.name;
  };

  const rows = sessions.state === "ready" ? pageRows(sessions.sessions, tab, search ?? "") : [];
  return (
    <section ref={page} className="sessions-page" {...leaving}>
      <div className="sessions-page-column">
        <h1 className="sessions-page-heading">{t("rail.sessionsPage.sessionsHeading")}</h1>
        <div className="sessions-page-bar">
          <div className="sessions-page-tabs" role="tablist" aria-label={t("rail.sessionsPage.tabsLabel")} onKeyDown={tabKey}>
            {TABS.map(({ tab: each, name }) => (
              <button
                key={each}
                ref={(el) => {
                  tabButtons.current[each] = el;
                }}
                type="button"
                role="tab"
                id={`${tabsId}-${each}`}
                className="sessions-page-tab"
                data-force={forceTab?.tab === each ? forceTab.state : undefined}
                aria-selected={each === tab}
                aria-controls={`${tabsId}-panel`}
                // The chosen tab is the one stop in Tab order; the arrow keys move between them.
                tabIndex={each === tab ? 0 : -1}
                onClick={() => setTab(each)}
              >
                {t(name)}
              </button>
            ))}
          </div>
          <div className="sessions-page-actions">
            {searching ? (
              <input
                ref={field}
                className="sessions-page-search"
                data-force={forceSearch ? "focus" : undefined}
                type="search"
                aria-label={t("rail.sessionsPage.searchLabel")}
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key !== "Escape") return;
                  event.preventDefault();
                  focusNext.current = "button";
                  setSearch(undefined);
                }}
              />
            ) : (
              <IconButton
                ref={searchButton}
                icon={Search}
                size="icon-sm"
                label={t("rail.sessionsPage.searchLabel")}
                onClick={() => {
                  focusNext.current = "field";
                  setSearch("");
                }}
              />
            )}
            <Button className="sessions-page-new" onClick={onNewSession} disabled={handlers.offline}>
              <Icon icon={Plus} />
              {t("rail.top.newSessionButton")}
            </Button>
          </div>
        </div>
        <div id={`${tabsId}-panel`} className="sessions-page-list" role="tabpanel" aria-labelledby={`${tabsId}-${tab}`}>
          {/* Read as the tree's notes do, where the rows would be (FLOW-ORGANIZE-SESSIONS-R19). */}
          {sessions.state === "loading" && <p className="rail-note">{t("rail.tree.loadingProgress")}</p>}
          {sessions.state === "error" && <Problem lead={t("rail.tree.sessionsError")} reason={sessions.message} />}
          {sessions.state === "unavailable" && <Problem lead={t("rail.tree.sessionsError")} reason={t("rail.sessionsPage.unavailableReason")} />}
          {sessions.state === "ready" && rows.length === 0 && (
            <p className="rail-note">{search ? t("rail.sessionsPage.noMatchEmpty") : tab === "archived" ? t("rail.tree.noArchivedEmpty") : t("rail.tree.noActiveEmpty")}</p>
          )}
          {rows.length > 0 && (
            <ul className="sessions-page-rows">
              {rows.map((s) => (
                <li key={s.id} className="session-item sessions-page-row" data-session-id={s.id}>
                  {/* A row that fails to draw takes only itself down; a new listing of the session tries again. */}
                  <ErrorBoundary what={t("rail.tile.boundaryName")} resetKey={JSON.stringify(s)}>
                    <SessionRow
                      session={s}
                      backends={backends}
                      project={projectOf(s)}
                      host={hostOf(s)}
                      time={lastActive(s, now)}
                      actions={tileActions(s, handlers)}
                      forking={handlers.forking?.[s.id]?.state === "sending"}
                      onOpen={onOpen}
                      force={force?.id === s.id ? force.state : undefined}
                      initialMenu={initialTileMenu === s.id}
                      initialDraft={initialTileDraft?.id === s.id ? initialTileDraft.draft : undefined}
                    />
                  </ErrorBoundary>
                  <Refusals session={s} pinning={handlers.pinning} archiving={handlers.archiving} forking={handlers.forking} />
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </section>
  );
}

/** One row: the title and, at the trailing side, the mark its tile would show, its project and host, and its time. */
function SessionRow({
  session,
  backends,
  project,
  host,
  time,
  actions,
  onOpen,
  force,
  initialMenu,
  initialDraft,
  forking,
}: {
  session: Session;
  backends?: Backends;
  project?: string;
  host?: string;
  time: string;
  actions: TileActions;
  onOpen: (id: string) => void;
  force?: ForcedTile["state"];
  initialMenu?: boolean;
  initialDraft?: string;
  /** A fork of this session is on its way: it says so in place of its state. */
  forking?: boolean;
}) {
  const view = tileOf(session, backends, forking);
  const { root, editing, rootHandlers, titleField, moreAndMenu } = useTileMenu({ session, title: view.title, actions, initialMenu, initialDraft });
  return (
    <>
      <SessionRoot ref={root} editing={editing} className="session" data-force={force} onClick={() => onOpen(session.id)} {...rootHandlers}>
        {editing ? titleField : <span className="sessions-page-title">{view.title}</span>}
        <span className="sessions-page-facts">
          {view.mark && (
            // A row has no line two to say what the mark means, so a mark that leaves that to line two is named here.
            <span className="sessions-page-mark" role={view.markLabel ? undefined : "img"} aria-label={view.markLabel ? undefined : view.phrase}>
              <Mark view={view} />
            </span>
          )}
          {project !== undefined && <span className="sessions-page-fact">{project}</span>}
          {host !== undefined && <span className="sessions-page-fact">{host}</span>}
          <span className="sessions-page-time">{time}</span>
        </span>
      </SessionRoot>
      {moreAndMenu}
    </>
  );
}
