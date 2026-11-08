import { type CSSProperties, type FocusEvent, type ReactNode, useCallback, useId, useLayoutEffect, useRef, useState } from "react";
import { ChevronDown, Ellipsis, Pin, Plus, ShieldAlert } from "lucide-react";
import { t } from "../catalog";
import type { ProjectsView, SessionsView } from "../core/connection";
import type { Project, Session, Workspace } from "../protocol/core-api.gen";
import type { Sending } from "../session/follow";
import { type Collapsed, collapsedOf, treeOf } from "../session/tree";
import { ErrorBoundary } from "./ErrorBoundary";
import { arrive, placesOf, slideFrom } from "../theme/motion";
import { Icon, IconButton } from "./Icon";
import { Menu, type MenuClose } from "./Menu";
import { type Backends, type ForcedTile, type OpenShelf, SessionTile, TileItem, type TileArchive, type TilePin, type TileRename } from "./SessionTile";
import { TrustQuestion } from "./TrustQuestion";
import "./RailTree.css";

interface RailTreeProps {
  projects: ProjectsView;
  sessions: SessionsView;
  /** Every known workspace, by id: a session sits under its workspace's project. */
  workspaces: Readonly<Record<string, Pick<Workspace, "project_id">>>;
  /** Why a project's workspaces could not be read, by project id. */
  workspaceErrors?: Readonly<Record<string, string>>;
  /** Some project's workspaces are still being read: a session not yet placed waits, rather than sit at the end. */
  placing?: boolean;
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
  /** The groups the user collapsed, by `groupKey` (LAY-SHELL, Collapsing); every other one is expanded. */
  collapsed?: ReadonlySet<string>;
  /** The user collapsed or expanded a group's heading, named by its `groupKey`. */
  onToggle?: (key: string) => void;
  /** Pins or unpins a session from its tile (FLOW-ORGANIZE-SESSIONS-R01). */
  onPin?: (id: string, pinned: boolean) => void;
  /** Pins and unpins on their way, and why the last was refused, by session id (R15). */
  pinning?: Readonly<Record<string, Sending>>;
  /** Renames a session through its backend from its tile; resolves true once Core has (FLOW-ORGANIZE-SESSIONS-R02). */
  onRename?: (id: string, title: string) => Promise<boolean>;
  /** Renames on their way, and why the last was refused, by session id. */
  renaming?: Readonly<Record<string, Sending>>;
  /** Editing a tile's title starts or ends: its earlier refusal goes. */
  onRenameReset?: (id: string) => void;
  /** Archives a session from its tile, or unarchives an archived one (FLOW-ARCHIVE-DELETE-SESSION, steps 1 and 3). */
  onArchive?: (id: string, archived: boolean) => void;
  /** Archives and unarchives on their way, and why the last was refused, by session id. */
  archiving?: Readonly<Record<string, Sending>>;
  /** Ersa can't reach Core, so nothing can be renamed or archived now. */
  offline?: boolean;
  /** For the gallery: a pointer or keyboard state on one tile or one heading, the menu open, the trust question asked. */
  force?: ForcedTile;
  forceHeading?: { id: string; state: "hover" | "focus" | "pressed" };
  initialMenu?: string;
  initialAsking?: string;
  /** For the gallery: the session whose tile menu is open, and a tile's title being edited. */
  initialTileMenu?: string;
  initialTileDraft?: { id: string; draft: string };
}

/** A group's key in `collapsed`: the Pinned group's, or a project's by its id. Kept apart, so no project id can be taken for the Pinned group. */
export const groupKey = { pinned: "pinned", project: (id: string) => `project:${id}` };

/**
 * The rail's tree (LAY-SHELL, The rail): the Pinned group, absent while nothing pinned is shown, then each project's
 * heading with its sessions beneath it, and their notes.
 */
export function RailTree(props: RailTreeProps) {
  const { projects, sessions, workspaces, placing, activeId, onSelect, backends, unseen, force, shelf, onChevron, onCloseShelf } = props;
  const { onRename, renaming, onRenameReset, onArchive, archiving, offline, onPin, pinning, initialTileMenu, initialTileDraft } = props;
  // Rename on a tile: unavailable only while Ersa can't reach Core. A session its backend has not named yet, such as one
  // still starting, is refused by Core, and the field says why.
  const renameOf = (s: Session): TileRename | undefined =>
    onRename && {
      onSave: (title) => onRename(s.id, title),
      renaming: renaming?.[s.id],
      onReset: () => onRenameReset?.(s.id),
      unavailable: offline ? t("rail.tile.offlineReason") : undefined,
    };
  const root = useRef<HTMLElement>(null);
  // The pin on its way: where every tile and heading sat when it was chosen, and whether focus was in the rail then.
  // One at a time: while it is saved, Pin and Unpin are unavailable on every tile, so each move is measured from the
  // rail as the last one left it.
  const pending = useRef<{ id: string; places: Map<string, number>; focused: boolean }>(undefined);
  const saving = Object.values(pinning ?? {}).some((p) => p.state === "sending");
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
  // read again after a reconnect, an unarchived session or a pinned one bring nothing new. A new session drawn at the
  // rail's end because its workspace isn't known yet keeps its slide for its place under its project.
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
    const unplaced = new Set(tree.loose.map((s) => s.id));
    const items = [...el.querySelectorAll<HTMLElement>("[data-session-id]")].filter((item) => {
      const id = item.dataset.sessionId ?? "";
      return arriving.current.has(id) && !unplaced.has(id);
    });
    if (items.length === 0) return;
    for (const item of items) arriving.current.delete(item.dataset.sessionId ?? "");
    arrive(el, items);
  });
  // Pin and Unpin: unavailable while Ersa can't reach Core, or while Core stores a pin.
  const pinOf = (s: Session): TilePin | undefined =>
    onPin && {
      pinned: s.pinned,
      onChoose: () => {
        const el = root.current;
        // Only a choice made with focus in the rail (in the tile's menu) has focus to give back.
        if (el) pending.current = { id: s.id, places: placesOf(el), focused: el.contains(document.activeElement) };
        onPin(s.id, !s.pinned);
      },
      unavailable: offline ? t("rail.tile.offlineReason") : saving ? t("rail.tile.savingProgress") : undefined,
    };
  const archiveOf = (s: Session): TileArchive | undefined =>
    onArchive && { onChoose: (archived) => onArchive(s.id, archived), unavailable: offline ? t("rail.tile.offlineReason") : undefined };
  // A tile that leaves the rail while keyboard focus is in it, as one archived from its menu does, hands focus to the
  // next tile, else the previous one, else New session (docs/design/RAIL.md, the rule after a deletion), unless the
  // user has put focus somewhere else meanwhile. The tiles drawn last time give the order.
  const drawn = useRef<string[]>([]);
  const focusedTile = useRef<string | undefined>(undefined);
  const trackFocus = (event: FocusEvent) => {
    focusedTile.current = (event.target as Element).closest<HTMLElement>("[data-session-id]")?.dataset.sessionId;
  };
  useLayoutEffect(() => {
    const section = root.current;
    if (!section) return;
    const items = [...section.querySelectorAll<HTMLElement>("[data-session-id]")];
    const ids = items.map((el) => el.dataset.sessionId ?? "");
    const before = drawn.current;
    drawn.current = ids;
    const gone = focusedTile.current;
    if (gone === undefined || ids.includes(gone)) return;
    focusedTile.current = undefined;
    const now = document.activeElement;
    if (now !== null && now !== document.body) return;
    const at = before.indexOf(gone);
    const after = before.slice(at + 1).find((id) => ids.includes(id));
    const earlier = before.slice(0, Math.max(at, 0)).reverse().find((id) => ids.includes(id));
    const to = after ?? earlier;
    const target = to !== undefined ? items[ids.indexOf(to)]?.querySelector<HTMLElement>(".session") : section.closest(".rail")?.querySelector<HTMLElement>(".rail-new-session");
    target?.focus();
  });
  const listed = projects.state === "ready" ? projects.projects : [];
  const tree = treeOf(listed, workspaces, sessions.state === "ready" ? sessions.sessions : []);
  // A session waits to be placed while the projects or their workspaces are still being read, so it does not
  // sit at the end and then move. When the projects could not be read, every session sits at the end.
  const looseShown = projects.state === "error" || (projects.state === "ready" && !placing);
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
    const pinFailed = pinning?.[s.id];
    const archive = archiving?.[s.id];
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
              rename={renameOf(s)}
              pin={pinOf(s)}
              archive={archiveOf(s)}
              initialMenu={initialTileMenu === s.id}
              initialDraft={initialTileDraft?.id === s.id ? initialTileDraft.draft : undefined}
            />
            {/* A refused archive or unarchive says why beneath the tile, which stays as it was. */}
            {archive?.state === "failed" && (
              <Problem lead={s.archived ? t("rail.tile.unarchiveError") : t("rail.tile.archiveError")} reason={archive.message} />
            )}
          </ErrorBoundary>
          {/* Core refused a pin: the tile stays where Core has it, and this says why (FLOW-ORGANIZE-SESSIONS-R15). */}
          {pinFailed?.state === "failed" && (
            <Problem lead={s.pinned ? t("rail.tile.unpinError") : t("rail.tile.pinError")} reason={pinFailed.message} />
          )}
          </>
        )}
      </TileItem>
    );
  };
  // The Pinned group leads the rail, and is absent while nothing pinned is shown (LAY-SHELL, Groups).
  const pinned = folded(groupKey.pinned, tree.pinned);
  const pinnedEmpty = tree.pinned.length === 0;
  return (
    <section ref={root} className="rail-tree" aria-label={t("rail.tree.label")}
      onFocus={trackFocus}
      // Focus put outside the rail is the user's: a tile leaving later takes nothing back.
      onBlur={(event) => {
        if (event.relatedTarget !== null && !root.current?.contains(event.relatedTarget)) focusedTile.current = undefined;
      }}
    >
      {projects.state === "loading" && <p className="rail-note">{t("rail.tree.loadingProgress")}</p>}
      {projects.state === "error" && <Problem lead={t("rail.tree.projectsError")} reason={projects.message} />}
      {sessions.state === "error" && <Problem lead={t("rail.tree.sessionsError")} reason={sessions.message} />}
      {projects.state === "ready" && listed.length === 0 && (
        <p className="rail-note">
          {t("rail.tree.noProjectsEmpty")}
          <span className="rail-note-detail">{t("rail.tree.noProjectsHint")}</span>
        </p>
      )}
      {(tree.pinned.length > 0 || tree.projects.length > 0) && (
        <ul className="rail-projects">
          {/* Always drawn, so it can open and fade in as the first session is pinned and close and fade out as the
              last is unpinned (DESIGN.md: nothing pops in). Empty, it is closed, out of reach and not read. */}
          <li className="rail-pinned" data-empty={pinnedEmpty || undefined} inert={pinnedEmpty || undefined} aria-hidden={pinnedEmpty || undefined}>
            <div className="rail-project" data-collapsed={pinned.folded || undefined}>
              <div className="rail-heading" data-slide={groupKey.pinned}>
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
                <ul className="rail-tiles">{tree.pinned.map(tile)}</ul>
              </GroupTiles>
            </div>
          </li>
          {tree.projects.map(({ project, sessions }) => {
            const closed = folded(groupKey.project(project.id), sessions);
            return (
              <ProjectGroup key={project.id} project={project} {...props} folded={closed.folded} summary={collapsedOf(sessions)} toggle={closed.toggle}>
                {sessions.length > 0 && <ul className="rail-tiles">{sessions.map(tile)}</ul>}
              </ProjectGroup>
            );
          })}
        </ul>
      )}
      {looseShown && tree.loose.length > 0 && <ul className="rail-tiles">{tree.loose.map(tile)}</ul>}
    </section>
  );
}

/** A note that something could not be read or done: what, after the danger glyph, and why beneath it. */
function Problem({ lead, reason }: { lead: string; reason: string }) {
  return (
    <div className="rail-note" role="alert">
      <p className="problem">{lead}</p>
      <p className="rail-note-detail rail-note-reason">{reason}</p>
    </div>
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
      <div className="rail-heading" data-slide={groupKey.project(project.id)} data-force={forced} data-open={menu ? true : undefined}>
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
