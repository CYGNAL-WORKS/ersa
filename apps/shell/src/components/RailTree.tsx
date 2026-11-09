import { type CSSProperties, type ReactNode, useCallback, useId, useLayoutEffect, useRef, useState } from "react";
import { ChevronDown, Ellipsis, Pin, Plus, ShieldAlert } from "lucide-react";
import { t } from "../catalog";
import type { ProjectsView, SessionsView } from "../core/connection";
import type { Project, Session, Workspace } from "../protocol/core-api.gen";
import type { Sending } from "../session/follow";
import { type ArrangeProblem, DEFAULT_ARRANGEMENT, type GroupBy, type Show, type SortBy } from "../session/arrangement";
import { type Collapsed, type DateKey, collapsedOf, sortValueOf, treeOf } from "../session/tree";
import { ErrorBoundary } from "./ErrorBoundary";
import { type Place, arrive, placesOf, regroup, slideFrom } from "../theme/motion";
import { Icon, IconButton } from "./Icon";
import { Menu, type MenuClose } from "./Menu";
import { type Backends, type ForcedTile, type OpenShelf, SessionTile, TileItem } from "./SessionTile";
import { type TileActions, type TileHandlers, tileActions } from "./TileMenu";
import { useLeavingFocus } from "./useLeavingFocus";
import { TrustQuestion } from "./TrustQuestion";
import "./RailTree.css";

interface RailTreeProps extends TileHandlers {
  projects: ProjectsView;
  sessions: SessionsView;
  /** Every known workspace, by id: a session sits under its workspace's project. */
  workspaces: Readonly<Record<string, Pick<Workspace, "project_id">>>;
  /** Why a project's workspaces could not be read, by project id. */
  workspaceErrors?: Readonly<Record<string, string>>;
  /** Some project's workspaces are still being read: a session not yet placed waits, rather than sit at the end. */
  placing?: boolean;
  /** The projects are being read again for a workspace no read had listed: a new session in one waits for the answer. */
  reading?: boolean;
  activeId?: string;
  onSelect: (id: string) => void;
  /** Opens the new-session screen for the project; nothing is created until its first message (FLOW-NEW-SESSION-R03). */
  onNewSession?: (project: Project) => void;
  /** Changes whether the user trusts the project. Trusting asks first, saying what trust lets run; withdrawing trust does not. */
  onSetTrust?: (project: Project, trusted: boolean) => void;
  /** Trust being changed, by project id, and why it could not be. */
  trusting?: Readonly<Record<string, Sending>>;
  /** The projects whose trust was withdrawn while sessions that started trusted still run, which keep it. */
  keepingTrust?: readonly string[];
  /** The local machine's host id: a project on any other host is named with its host. */
  localHost?: string;
  /** Each host's name, by id; a host not listed is named by its id. */
  hostNames?: Readonly<Record<string, string>>;
  /** Each host's backends, to name a tile's backend by its own name; until they are read, it is named by its id. */
  backends?: Backends;
  /** The unseen sessions' ids (LAY-SHELL, Unseen): their titles read strong until they are made active. */
  unseen?: ReadonlySet<string>;
  /** The open shelf: its session, its tone, and the shelf itself, which is drawn beside that session's tile. */
  shelf?: OpenShelf;
  /** The user chose a tile's chevron (FLOW-APPROVE-PERMISSION-R01). */
  onChevron?: (id: string) => void;
  /** The user closed a session's shelf: Escape, or a choice or focus outside it and its tile (R06, R07). */
  onCloseShelf?: (id: string) => void;
  /** How the rail groups and sorts and which sessions it shows (FLOW-ORGANIZE-SESSIONS); by project, by date created, showing active ones, unless given. */
  groupBy?: GroupBy;
  sortBy?: SortBy;
  show?: Show;
  /** Now, whose local date the date groups count from. */
  now?: Date;
  /** The groups the user collapsed in the grouping in use, by key (LAY-SHELL, Collapsing); every other one is expanded. */
  collapsed?: ReadonlySet<string>;
  /** The user collapsed or expanded a group's heading, named by its key. */
  onToggle?: (key: string) => void;
  /** The arrangement could not be read, so the rail shows the defaults, or a change to it could not be saved, so it was undone (R15). */
  arrangeProblem?: ArrangeProblem;
  /** For the gallery: a pointer or keyboard state on one tile or one heading, the menu open, the trust question asked. */
  force?: ForcedTile;
  forceHeading?: { id: string; state: "hover" | "focus" | "pressed" };
  initialMenu?: string;
  initialAsking?: string;
  /** For the gallery: the session whose tile menu is open, and a tile's title being edited. */
  initialTileMenu?: string;
  initialTileDraft?: { id: string; draft: string };
}

/** The Pinned group's key in `collapsed` (PROTO-CORE-API, Settings); every other group's is its `Group.key`. */
export const PINNED_KEY = "pinned";

/** A heading's mark for the slide when the rail regroups: apart from the tiles' `session:` marks. */
const headingSlide = (key: string) => `heading:${key}`;

/**
 * The rail's tree (LAY-SHELL, The rail): the Pinned group, absent while nothing pinned is shown, then each project's
 * heading with its sessions beneath it, and their notes.
 */
export function RailTree(props: RailTreeProps) {
  const { projects, sessions, workspaces, placing, reading, activeId, onSelect, backends, unseen, force, shelf, onChevron, onCloseShelf } = props;
  const { archiving, pinning, initialTileMenu, initialTileDraft, arrangeProblem } = props;
  const root = useRef<HTMLElement>(null);
  // The pin on its way: where every tile and heading sat when it was chosen, and whether focus was in the rail then.
  // One at a time: while it is saved, Pin and Unpin are unavailable on every tile, so each move is measured from the
  // rail as the last one left it.
  const pending = useRef<{ id: string; places: Map<string, Place>; focused: boolean }>(undefined);
  useLayoutEffect(() => {
    const pin = pending.current;
    const el = root.current;
    if (!pin || !el || pinning?.[pin.id]?.state === "sending") return;
    pending.current = undefined;
    // Core refused it: nothing moved, so nothing slides.
    if (pinning?.[pin.id]?.state === "failed") return;
    // Once Core has the pin, each tile and heading slides from where it sat to its new place (DESIGN.md, Motion).
    slideFrom(el, pin.places);
    // The tile is drawn afresh in its new group, so the focus its menu gave back is lost: it goes to the tile in its
    // new place or, when that group is collapsed, to the group's heading.
    const lost = document.activeElement === null || document.activeElement === document.body;
    const item = [...el.querySelectorAll<HTMLElement>("[data-session-id]")].find((tile) => tile.dataset.sessionId === pin.id);
    if (!pin.focused || !lost || !item) return;
    const folded = item.closest<HTMLElement>("[inert]");
    const target = folded ? folded.closest(".rail-project")?.querySelector<HTMLElement>(".rail-heading-toggle") : item.querySelector<HTMLElement>(".session");
    target?.focus({ preventScroll: true });
  });
  // A new session's tile, a fork's included, slides in at its place while the others make room (DESIGN.md, Motion). New
  // means listed for the first time since Ersa started: the first listing draws every tile in place, and a listing
  // read again after a reconnect, an unarchived session or a pinned one bring nothing new. A new session still waiting
  // to be placed under its project slides in once it is drawn.
  const seen = useRef<Set<string>>(undefined);
  const arriving = useRef(new Set<string>());
  useLayoutEffect(() => {
    const el = root.current;
    if (sessions.state !== "ready" || !el) return;
    const ids = sessions.sessions.map((s) => s.id);
    const known = seen.current;
    seen.current = new Set([...(known ?? []), ...ids]);
    // An archived one has no tile to slide in, so it never arrives; unarchived later, it is not new.
    if (known) for (const s of sessions.sessions) if (!known.has(s.id) && !s.archived) arriving.current.add(s.id);
    // One that left the listing before it was drawn no longer arrives.
    for (const id of arriving.current) if (!ids.includes(id)) arriving.current.delete(id);
    const items = [...el.querySelectorAll<HTMLElement>("[data-session-id]")].filter((item) => arriving.current.has(item.dataset.sessionId ?? ""));
    if (items.length === 0) return;
    for (const item of items) arriving.current.delete(item.dataset.sessionId ?? "");
    arrive(el, items);
  });
  // Pin and Unpin first measure the rail, so each tile slides from where it sat once Core has the pin.
  const actionsOf = (s: Session): TileActions => {
    const actions = tileActions(s, props);
    const { pin } = actions;
    if (!pin) return actions;
    const onChoose = () => {
      const el = root.current;
      // Only a choice made with focus in the rail (in the tile's menu) has focus to give back.
      if (el) pending.current = { id: s.id, places: placesOf(el), focused: el.contains(document.activeElement) };
      pin.onChoose();
    };
    return { ...actions, pin: { ...pin, onChoose } };
  };
  const leaving = useLeavingFocus(root, (section) => section.closest(".rail")?.querySelector<HTMLElement>(".rail-new-session"));
  const { groupBy = DEFAULT_ARRANGEMENT.groupBy, sortBy = DEFAULT_ARRANGEMENT.sortBy, show = DEFAULT_ARRANGEMENT.show, now = new Date() } = props;
  // A new grouping or filter: where every tile and heading sat before it, taken while the page still shows the old
  // one (this render has not reached it yet), so each slides to its new place once it has (FLOW-ORGANIZE-SESSIONS,
  // step 4). So too when what the sort reads of a session differs from the listing before, as it does when the user
  // chooses another sort or a turn ends while sorted by last activity (DESIGN.md, Motion); a tile's state is no part
  // of it (R10).
  const arranged = `${groupBy}/${show}`;
  const lastArranged = useRef(arranged);
  const sortValues = sessions.state === "ready" ? new Map(sessions.sessions.map((s) => [s.id, sortValueOf(s, sortBy)])) : undefined;
  const lastSortValues = useRef(sortValues);
  const wereSorted = lastSortValues.current;
  lastSortValues.current = sortValues;
  const moves = useRef<{ from: Map<string, Place>; slide: typeof regroup }>(undefined);
  if (lastArranged.current !== arranged) {
    lastArranged.current = arranged;
    if (root.current) moves.current = { from: placesOf(root.current), slide: regroup };
  } else if (root.current && wereSorted && sortValues && [...sortValues].some(([id, value]) => wereSorted.has(id) && wereSorted.get(id) !== value)) {
    moves.current = { from: placesOf(root.current), slide: slideFrom };
  }
  useLayoutEffect(() => {
    const move = moves.current;
    moves.current = undefined;
    if (move && root.current) move.slide(root.current, move.from);
  });
  const listed = projects.state === "ready" ? projects.projects : [];
  const all = sessions.state === "ready" ? sessions.sessions : [];
  const tree = treeOf(listed, workspaces, all, { groupBy, sortBy, show, now });
  // Grouped by project, a session waits to be placed while the projects or their workspaces are still being read,
  // so it does not sit at the end and then move. When the projects could not be read, every session sits at the
  // end. Grouped otherwise, no session waits on its project.
  const looseShown = groupBy !== "project" || projects.state === "error" || (projects.state === "ready" && !placing);
  // Nothing the filter shows (FLOW-ORGANIZE-SESSIONS, Nothing to show; LAY-SHELL, Loading, empty and failed). Showing
  // active with no sessions at all, project headings say enough where there are any, and with no projects their own
  // note does. Showing all, it waits for the projects, so the note is not drawn beneath Loading and then jumps up.
  const nothing = sessions.state === "ready" && tree.pinned.length === 0 && tree.groups.every((g) => g.sessions.length === 0) && tree.loose.length === 0;
  const blank = projects.state === "ready" && listed.length > 0 && tree.groups.length === 0;
  const noneShown = !nothing
    ? undefined
    : show === "archived"
      ? t("rail.tree.noArchivedEmpty")
      : show === "active" && (all.length > 0 || blank)
        ? t("rail.tree.noActiveEmpty")
        : show === "all" && projects.state !== "loading"
          ? t("rail.tree.noSessionsEmpty")
          : undefined;
  // A new session whose workspace is being read waits too, so it slides in once, at its place. Tiles already drawn stay.
  const isNew = (s: Session) => seen.current !== undefined && (!seen.current.has(s.id) || arriving.current.has(s.id));
  const loose = tree.loose.filter((s) => !(reading && workspaces[s.workspace_id] === undefined && isNew(s)));
  // The Pinned group's tiles' element, which its heading's toggle says it controls.
  const pinnedTiles = useId();
  // Whether a group is collapsed, and how its heading turns it.
  const folded = (key: string, sessions: readonly Session[]) => {
    const closed = props.collapsed?.has(key) ?? false;
    const toggle = () => {
      // A shelf whose tile is folded away closes with it.
      if (!closed && shelf && sessions.some((s) => s.id === shelf.id)) onCloseShelf?.(shelf.id);
      props.onToggle?.(key);
    };
    return { folded: closed, toggle };
  };
  const tile = (s: Session) => {
    const open = shelf?.id === s.id ? shelf : undefined;
    return (
      <TileItem key={s.id} id={s.id} shelf={open} onClose={onCloseShelf}>
        {(shelfDomId) => (
          <>
          {/* A tile that fails to draw takes only itself down; a new listing of the session tries again. */}
          <ErrorBoundary what={t("rail.tile.boundaryName")} resetKey={JSON.stringify(s)}>
            <SessionTile
              session={s}
              backends={backends}
              active={s.id === activeId}
              unseen={unseen?.has(s.id) ?? false}
              force={force?.id === s.id ? force.state : undefined}
              onSelect={onSelect}
              shelf={open && { tone: open.tone, domId: shelfDomId }}
              onChevron={onChevron}
              {...actionsOf(s)}
              initialMenu={initialTileMenu === s.id}
              initialDraft={initialTileDraft?.id === s.id ? initialTileDraft.draft : undefined}
            />
          </ErrorBoundary>
          <Refusals session={s} pinning={pinning} archiving={archiving} />
          </>
        )}
      </TileItem>
    );
  };
  const tiles = (shown: readonly Session[]) => (
    <ul className="rail-tiles" ref={movesInPlace}>
      {shown.map(tile)}
    </ul>
  );
  // The Pinned group leads the rail, and is absent while nothing pinned is shown (LAY-SHELL, Groups).
  const pinned = folded(PINNED_KEY, tree.pinned);
  const pinnedEmpty = tree.pinned.length === 0;
  return (
    <section ref={root} className="rail-tree" aria-label={t("rail.tree.label")} {...leaving}>
      {projects.state === "loading" && <p className="rail-note">{t("rail.tree.loadingProgress")}</p>}
      {projects.state === "error" && <Problem lead={t("rail.tree.projectsError")} reason={projects.message} />}
      {sessions.state === "error" && <Problem lead={t("rail.tree.sessionsError")} reason={sessions.message} />}
      {projects.state === "ready" && listed.length === 0 && (
        <p className="rail-note">
          {t("rail.tree.noProjectsEmpty")}
          <span className="rail-note-detail">{t("rail.tree.noProjectsHint")}</span>
        </p>
      )}
      {arrangeProblem && (
        <Problem lead={arrangeProblem.kind === "read" ? t("rail.tree.arrangeReadError") : t("rail.tree.arrangeSaveError")} reason={arrangeProblem.message} />
      )}
      {/* Drawn once the sessions are read even with nothing in it, so the Pinned group's frame is there to open. */}
      {(sessions.state === "ready" || tree.groups.length > 0) && (
        <ul className="rail-projects">
          {/* Always drawn, so it can open and fade in as the first session is pinned and close and fade out as the
              last is unpinned (DESIGN.md: nothing pops in). Empty, it is closed, out of reach and not read. */}
          <li className="rail-pinned" data-empty={pinnedEmpty || undefined} inert={pinnedEmpty || undefined} aria-hidden={pinnedEmpty || undefined}>
            <div className="rail-project" data-collapsed={pinned.folded || undefined}>
              <div className="rail-heading" data-slide={headingSlide(PINNED_KEY)}>
                <GroupToggle
                  folded={pinned.folded}
                  controls={pinnedTiles}
                  summary={collapsedOf(tree.pinned)}
                  toggle={pinned.toggle}
                  name={t("rail.group.pinnedHeading")}
                >
                  <span className="rail-heading-pin">
                    <Icon icon={Pin} size="icon-sm" />
                  </span>
                </GroupToggle>
              </div>
              <GroupTiles id={pinnedTiles} folded={pinned.folded}>
                {tiles(tree.pinned)}
              </GroupTiles>
            </div>
          </li>
          {tree.groups.map((group) => {
            const closed = folded(group.key, group.sessions);
            const held = group.sessions.length > 0 && tiles(group.sessions);
            if (group.kind === "date")
              return (
                <DateGroup key={group.key} name={t(DATE_NAMES[group.key])} slide={headingSlide(group.key)} folded={closed.folded} summary={collapsedOf(group.sessions)} toggle={closed.toggle}>
                  {held}
                </DateGroup>
              );
            return (
              <ProjectGroup key={group.key} project={group.project} {...props} folded={closed.folded} summary={collapsedOf(group.sessions)} toggle={closed.toggle}>
                {held}
              </ProjectGroup>
            );
          })}
        </ul>
      )}
      {looseShown && loose.length > 0 && tiles(loose)}
      {noneShown && (
        <p key={noneShown} className="rail-note" data-slide={`note:${noneShown}`}>
          {noneShown}
        </p>
      )}
    </section>
  );
}

/**
 * For a list of tiles, as its ref: a tile React reorders is moved where it is, not taken off the page and put back,
 * since that would close its open menu, lose its focus and start its shelf's and glyph's animations over while another
 * session's turn ends. Without `moveBefore`, React inserts as it always does, and the tile still reaches its place.
 */
function movesInPlace(list: HTMLUListElement | null): void {
  if (!list || typeof list.moveBefore !== "function") return;
  list.insertBefore = <T extends Node>(node: T, before: Node | null): T => {
    if (node.parentNode === list) list.moveBefore(node, before);
    else Node.prototype.insertBefore.call(list, node, before);
    return node;
  };
  list.appendChild = <T extends Node>(node: T): T => list.insertBefore(node, null);
}

/** Each date group's heading (FLOW-ORGANIZE-SESSIONS-R07). */
const DATE_NAMES = {
  today: "rail.group.todayHeading",
  yesterday: "rail.group.yesterdayHeading",
  previous_7_days: "rail.group.previous7DaysHeading",
  older: "rail.group.olderHeading",
} as const satisfies Record<DateKey, string>;

/** A date group: its heading, with no actions (docs/design/RAIL.md, A group heading), and its tiles. */
function DateGroup({ name, slide, folded, summary, toggle, children }: { name: string; slide: string; folded: boolean; summary: Collapsed; toggle: () => void; children: ReactNode }) {
  const tilesId = useId();
  return (
    <li className="rail-project" data-collapsed={folded || undefined}>
      <div className="rail-heading" data-slide={slide}>
        <GroupToggle folded={folded} controls={tilesId} summary={summary} toggle={toggle} name={name} />
      </div>
      <GroupTiles id={tilesId} folded={folded}>
        {children}
      </GroupTiles>
    </li>
  );
}

/** A note that something could not be read or done: what, after the danger glyph, and why beneath it. */
export function Problem({ lead, reason }: { lead: string; reason: string }) {
  return (
    <div className="rail-note" role="alert">
      <p className="problem">{lead}</p>
      <p className="rail-note-detail rail-note-reason">{reason}</p>
    </div>
  );
}

/** Why Core refused a session's pin, archive or unarchive, beneath its tile or row, which stays as Core has it (R15). */
export function Refusals({
  session,
  pinning,
  archiving,
}: {
  session: Session;
  pinning?: Readonly<Record<string, Sending>>;
  archiving?: Readonly<Record<string, Sending>>;
}) {
  const pin = pinning?.[session.id];
  const archive = archiving?.[session.id];
  return (
    <>
      {archive?.state === "failed" && <Problem lead={session.archived ? t("rail.tile.unarchiveError") : t("rail.tile.archiveError")} reason={archive.message} />}
      {pin?.state === "failed" && <Problem lead={session.pinned ? t("rail.tile.unpinError") : t("rail.tile.pinError")} reason={pin.message} />}
    </>
  );
}

/** A project: its heading, what changing its trust says, and its tiles. */
function ProjectGroup({
  project,
  onNewSession,
  onSetTrust,
  trusting = {},
  keepingTrust = [],
  workspaceErrors = {},
  localHost,
  hostNames,
  forceHeading,
  initialMenu,
  initialAsking,
  folded,
  summary,
  toggle,
  children,
}: RailTreeProps & {
  project: Project;
  /** Collapsed: its tiles are folded away. */
  folded: boolean;
  /** What its heading says of the sessions it holds while collapsed. */
  summary: Collapsed;
  toggle: () => void;
  children: ReactNode;
}) {
  // The tiles' element, which the heading's toggle says it controls.
  const tilesId = useId();
  // The More menu, while open: where focus went as it opened (RAIL.md, Menus). The gallery's open menu takes none.
  const [menu, setMenu] = useState<"item" | "menu" | "none" | undefined>(initialMenu === project.id ? "none" : undefined);
  const [asking, setAsking] = useState(initialAsking === project.id);
  const more = useRef<HTMLButtonElement>(null);
  // The menu floats above the rail in the top layer, anchored to More by this name.
  const anchor = `--rail-more-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  const trust = trusting[project.id];
  const saving = trust?.state === "sending";
  const host = localHost !== undefined && project.host_id !== localHost ? (hostNames?.[project.host_id] ?? project.host_id) : undefined;
  const forced = forceHeading?.id === project.id ? forceHeading.state : undefined;

  // Once the menu or the trust question closes, focus goes back to More, which opened it.
  const back = () => more.current?.focus({ preventScroll: true });
  const closeMenu = useCallback((how: MenuClose) => {
    setMenu(undefined);
    if (how !== "away") back();
  }, []);
  const insideMenu = useCallback(() => [more.current], []);
  const choose = () => {
    if (!onSetTrust) return;
    if (project.trusted) onSetTrust(project, false);
    // The question takes focus as it opens.
    else setAsking(true);
  };
  const closeQuestion = () => {
    setAsking(false);
    back();
  };

  const nameLabel = { project: project.name };
  return (
    <li className="rail-project" data-collapsed={folded || undefined}>
      <div className="rail-heading" data-slide={headingSlide(project.id)} data-force={forced} data-open={menu ? true : undefined}>
        <GroupToggle
          folded={folded}
          controls={children ? tilesId : undefined}
          summary={summary}
          toggle={toggle}
          name={project.name}
          host={host}
          notTrusted={!project.trusted}
        />
        <span className="rail-heading-actions">
          {onNewSession && <IconButton icon={Plus} size="icon-sm" label={t("rail.heading.newSessionLabel", nameLabel)} onClick={() => onNewSession(project)} />}
          {onSetTrust && (
            <IconButton
              ref={more}
              icon={Ellipsis}
              size="icon-sm"
              label={t("rail.heading.moreLabel", nameLabel)}
              aria-haspopup="menu"
              aria-expanded={!!menu}
              style={{ anchorName: anchor } as CSSProperties}
              // A click the keyboard made (Enter or Space) has no pointer presses to count.
              onClick={(event) => setMenu(menu ? undefined : event.detail === 0 ? "item" : "menu")}
            />
          )}
        </span>
        {onSetTrust && (
          <Menu
            label={t("rail.heading.moreLabel", nameLabel)}
            open={menu !== undefined}
            place={{ anchor }}
            focus={menu}
            inside={insideMenu}
            onClose={closeMenu}
            entries={[
              {
                kind: "item",
                label: project.trusted ? t("rail.heading.distrustButton") : t("rail.heading.trustButton"),
                unavailable: saving ? t("rail.heading.savingProgress") : undefined,
                onChoose: choose,
              },
            ]}
          />
        )}
      </div>
      {asking && onSetTrust && (
        <TrustQuestion
          name={project.name}
          path={project.path}
          purpose="change"
          takeFocus={initialAsking !== project.id}
          onTrust={() => {
            closeQuestion();
            onSetTrust(project, true);
          }}
          onDismiss={closeQuestion}
        />
      )}
      {/* Always there, so a screen reader announces the text when it appears. */}
      <p className="rail-project-note" role="status">
        {saving ? t("rail.heading.savingProgress") : keepingTrust.includes(project.id) ? t("rail.heading.keepTrust") : ""}
      </p>
      {trust?.state === "failed" && <Problem lead={t("rail.heading.trustError")} reason={trust.message} />}
      {workspaceErrors[project.id] !== undefined && <Problem lead={t("rail.heading.workspacesError")} reason={workspaceErrors[project.id] ?? ""} />}
      {/* The tiles slide closed and open in this frame; folded away, they take no focus and are not read. */}
      {children && (
        <GroupTiles id={tilesId} folded={folded}>
          {children}
        </GroupTiles>
      )}
    </li>
  );
}

/**
 * A group heading's name, which collapses and expands its tiles and says which it is (LAY-SHELL, Collapsing): the
 * chevron, what leads the name (the Pinned group's pin), the name and its host, and toward the trailing edge "Not
 * trusted" and, collapsed, how many sessions it holds.
 */
function GroupToggle({
  folded,
  controls,
  summary,
  toggle,
  name,
  host,
  notTrusted = false,
  children,
}: {
  folded: boolean;
  /** The tiles' element id, when it has tiles. */
  controls?: string;
  summary: Collapsed;
  toggle: () => void;
  name: string;
  host?: string;
  notTrusted?: boolean;
  /** Drawn after the chevron, before the name. */
  children?: ReactNode;
}) {
  // The "Not trusted" tooltip is placed by the shield, which names it as its anchor.
  const shieldAnchor = `--rail-shield-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  return (
    <h3 className="rail-heading-title">
      {/* The heading's name collapses and expands its tiles, and says which it is (LAY-SHELL, Collapsing). */}
      <button type="button" className="rail-heading-toggle" aria-expanded={!folded} aria-controls={controls} onClick={toggle}>
        <span className="rail-heading-chevron">
          <Icon icon={ChevronDown} size="icon-sm" />
        </span>
        {children}
        <span className="rail-heading-name">{name}</span>
        {host !== undefined && <span className="rail-heading-host">{host}</span>}
        {/* Toward the trailing edge: "Not trusted" after its shield while expanded; collapsed, the shield alone,
            then how many sessions it holds. The words and the count fade as the heading turns (RailTree.css). */}
        <span className="rail-heading-status">
          {notTrusted && (
            <span
              className="rail-heading-shield"
              style={{ anchorName: shieldAnchor } as CSSProperties}
              // Collapsed, the shield stands for the words, so it carries them to assistive technology.
              role={folded ? "img" : undefined}
              aria-label={folded ? t("trust.notTrustedStatus") : undefined}
            >
              <Icon icon={ShieldAlert} size="icon-sm" />
            </span>
          )}
          <span className="rail-heading-swap">
            {notTrusted && <span className="rail-heading-trust">{t("trust.notTrustedStatus")}</span>}
            <span className="rail-heading-count">
              {/* Led by a mark in the tone of what they want, when any of them needs the user. */}
              {summary.tone && <span className="glyph" data-tone={summary.tone} role="img" aria-label={t("rail.tile.needsYouWant")} />}
              {t("rail.heading.sessionCountStatus", { count: summary.count })}
            </span>
          </span>
          {notTrusted && folded && (
            <span className="rail-heading-tip" aria-hidden="true" style={{ positionAnchor: shieldAnchor } as CSSProperties}>
              {t("trust.notTrustedStatus")}
            </span>
          )}
        </span>
      </button>
    </h3>
  );
}

/** A group's tiles, which slide closed and open in this frame; folded away, they take no focus and are not read. */
function GroupTiles({ id, folded, children }: { id: string; folded: boolean; children: ReactNode }) {
  return (
    <div id={id} className="rail-group-tiles" inert={folded || undefined}>
      <div className="rail-group-tiles-inner">{children}</div>
    </div>
  );
}
