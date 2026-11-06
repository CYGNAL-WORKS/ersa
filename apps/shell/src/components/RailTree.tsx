import { type CSSProperties, type FocusEvent, type KeyboardEvent, type ReactNode, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { ChevronDown, Ellipsis, Plus, ShieldAlert } from "lucide-react";
import { t } from "../catalog";
import type { ProjectsView, SessionsView } from "../core/connection";
import type { Project, Session, Workspace } from "../protocol/core-api.gen";
import type { Sending } from "../session/follow";
import { type Collapsed, collapsedOf, treeOf } from "../session/tree";
import { ErrorBoundary } from "./ErrorBoundary";
import { Icon, IconButton } from "./Icon";
import { type Backends, type ForcedTile, type OpenShelf, SessionTile, TileItem } from "./SessionTile";
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
  /** The projects whose headings the user collapsed (LAY-SHELL, Collapsing); every other one is expanded. */
  collapsed?: ReadonlySet<string>;
  /** The user collapsed or expanded a project's heading. */
  onToggle?: (projectId: string) => void;
  /** For the gallery: a pointer or keyboard state on one tile or one heading, the menu open, the trust question asked. */
  force?: ForcedTile;
  forceHeading?: { id: string; state: "hover" | "focus" | "pressed" };
  initialMenu?: string;
  initialAsking?: string;
}

/** The rail's tree (LAY-SHELL, The rail): each project's heading with its sessions beneath it, and their notes. */
export function RailTree(props: RailTreeProps) {
  const { projects, sessions, workspaces, placing, activeId, onSelect, backends, unseen, force, shelf, onChevron, onCloseShelf } = props;
  const listed = projects.state === "ready" ? projects.projects : [];
  const tree = treeOf(listed, workspaces, sessions.state === "ready" ? sessions.sessions : []);
  // A session waits to be placed while the projects or their workspaces are still being read, so it does not
  // sit at the end and then move. When the projects could not be read, every session sits at the end.
  const looseShown = projects.state === "error" || (projects.state === "ready" && !placing);
  const tile = (s: Session) => {
    const open = shelf?.id === s.id ? shelf : undefined;
    return (
      <TileItem key={s.id} id={s.id} shelf={open} onClose={onCloseShelf}>
        {(shelfDomId) => (
          // A tile that fails to draw takes only itself down; a new listing of the session tries again.
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
            />
          </ErrorBoundary>
        )}
      </TileItem>
    );
  };
  return (
    <section className="rail-tree" aria-label={t("rail.tree.label")}>
      {projects.state === "loading" && <p className="rail-note">{t("rail.tree.loadingProgress")}</p>}
      {projects.state === "error" && <Problem lead={t("rail.tree.projectsError")} reason={projects.message} />}
      {sessions.state === "error" && <Problem lead={t("rail.tree.sessionsError")} reason={sessions.message} />}
      {projects.state === "ready" && listed.length === 0 && (
        <p className="rail-note">
          {t("rail.tree.noProjectsEmpty")}
          <span className="rail-note-detail">{t("rail.tree.noProjectsHint")}</span>
        </p>
      )}
      {tree.projects.length > 0 && (
        <ul className="rail-projects">
          {tree.projects.map(({ project, sessions }) => {
            const closed = props.collapsed?.has(project.id) ?? false;
            const toggle = () => {
              // A shelf whose tile is folded away closes with it.
              if (!closed && shelf && sessions.some((s) => s.id === shelf.id)) onCloseShelf?.(shelf.id);
              props.onToggle?.(project.id);
            };
            return (
              <ProjectGroup key={project.id} project={project} {...props} folded={closed} summary={collapsedOf(sessions)} toggle={toggle}>
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
  // The "Not trusted" tooltip is placed by the shield, which names it as its anchor.
  const shieldAnchor = `--rail-shield-${tilesId.replace(/[^a-zA-Z0-9]/g, "")}`;
  const [menu, setMenu] = useState(initialMenu === project.id);
  const [asking, setAsking] = useState(initialAsking === project.id);
  const more = useRef<HTMLButtonElement>(null);
  // The menu floats above the rail in the top layer, anchored to More by this name (RailTree.css, .rail-menu).
  const anchor = `--rail-more-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  const trust = trusting[project.id];
  const saving = trust?.state === "sending";
  const host = localHost !== undefined && project.host_id !== localHost ? (hostNames?.[project.host_id] ?? project.host_id) : undefined;
  const forced = forceHeading?.id === project.id ? forceHeading.state : undefined;

  // A press anywhere but on More or the menu closes it, leaving focus where the press put it: New session and the
  // heading's name too. More's own press toggles it.
  const list = useRef<HTMLUListElement>(null);
  useEffect(() => {
    if (!menu) return;
    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!more.current?.contains(target) && !list.current?.contains(target)) setMenu(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [menu]);

  // The menu stays drawn while it fades out, then goes (RAIL.md, Menus): `shown` outlasts `menu` until its
  // animations finish, at once when there are none, as with reduced motion.
  const [shown, setShown] = useState(menu);
  if (menu && !shown) setShown(true);
  useLayoutEffect(() => {
    const el = list.current;
    if (!el) return;
    // A popover that is not open is not drawn; showing it puts it in the top layer, above the rail's scrolling.
    if (!el.matches(":popover-open")) el.showPopover();
    if (menu) return;
    let live = true;
    Promise.all(el.getAnimations().map((a) => a.finished)).then(
      () => live && setShown(false),
      () => live && setShown(false),
    );
    return () => {
      live = false;
    };
  }, [menu, shown]);

  // The gallery's open menu does not take focus; one the user opens does.
  // Opened from the keyboard, focus goes to the item; from the pointer, to the menu (RAIL.md, Menus).
  const focusOnOpen = useRef<"item" | "menu">(undefined);
  useEffect(() => {
    const target = focusOnOpen.current;
    if (!menu || !target) return;
    focusOnOpen.current = undefined;
    const into = target === "item" ? list.current?.querySelector<HTMLElement>('[role="menuitem"]') : list.current;
    into?.focus({ preventScroll: true });
  }, [menu]);

  // Once the menu or the trust question closes, focus goes back to More, which opened it.
  const back = () => more.current?.focus({ preventScroll: true });
  const closeMenu = () => {
    setMenu(false);
    back();
  };
  // Escape closes the menu wherever focus is in the heading, and focus leaving the heading closes it too.
  const onHeadingKey = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!menu || event.key !== "Escape") return;
    event.preventDefault();
    // The menu is the heading's; Escape here is not also a request to leave the screen.
    event.stopPropagation();
    closeMenu();
  };
  const onHeadingBlur = (event: FocusEvent<HTMLDivElement>) => {
    if (menu && !event.currentTarget.contains(event.relatedTarget as Node | null)) setMenu(false);
  };
  const choose = () => {
    if (saving || !onSetTrust) return;
    if (project.trusted) {
      closeMenu();
      onSetTrust(project, false);
    } else {
      // The question takes focus as it opens.
      setMenu(false);
      setAsking(true);
    }
  };
  const closeQuestion = () => {
    setAsking(false);
    back();
  };

  const nameLabel = { project: project.name };
  return (
    <li className="rail-project" data-collapsed={folded || undefined}>
      <div className="rail-heading" data-force={forced} data-open={menu || undefined} onKeyDown={onHeadingKey} onBlur={onHeadingBlur}>
        <h3 className="rail-heading-title">
          {/* The heading's name collapses and expands its tiles, and says which it is (LAY-SHELL, Collapsing). */}
          <button type="button" className="rail-heading-toggle" aria-expanded={!folded} aria-controls={children ? tilesId : undefined} onClick={toggle}>
            <span className="rail-heading-chevron">
              <Icon icon={ChevronDown} size="icon-sm" />
            </span>
            <span className="rail-heading-name">{project.name}</span>
            {host !== undefined && <span className="rail-heading-host">{host}</span>}
            {/* Toward the trailing edge: "Not trusted" after its shield while expanded; collapsed, the shield alone,
                then how many sessions it holds. The words and the count fade as the heading turns (RailTree.css). */}
            <span className="rail-heading-status">
              {!project.trusted && (
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
                {!project.trusted && <span className="rail-heading-trust">{t("trust.notTrustedStatus")}</span>}
                <span className="rail-heading-count">
                  {/* Led by a mark in the tone of what they want, when any of them needs the user. */}
                  {summary.tone && <span className="glyph" data-tone={summary.tone} role="img" aria-label={t("rail.tile.needsYouWant")} />}
                  {t("rail.heading.sessionCountStatus", { count: summary.count })}
                </span>
              </span>
              {!project.trusted && folded && (
                <span className="rail-heading-tip" aria-hidden="true" style={{ positionAnchor: shieldAnchor } as CSSProperties}>
                  {t("trust.notTrustedStatus")}
                </span>
              )}
            </span>
          </button>
        </h3>
        <span className="rail-heading-actions">
          {onNewSession && <IconButton icon={Plus} size="icon-sm" label={t("rail.heading.newSessionLabel", nameLabel)} onClick={() => onNewSession(project)} />}
          {onSetTrust && (
            <IconButton
              ref={more}
              icon={Ellipsis}
              size="icon-sm"
              label={t("rail.heading.moreLabel", nameLabel)}
              aria-haspopup="menu"
              aria-expanded={menu}
              style={{ anchorName: anchor } as CSSProperties}
              onClick={(event) => {
                // A click the keyboard made (Enter or Space) has no pointer presses to count.
                focusOnOpen.current = menu ? undefined : event.detail === 0 ? "item" : "menu";
                setMenu(!menu);
              }}
            />
          )}
        </span>
        {shown && onSetTrust && (
          <ul
            ref={list}
            className="rail-menu"
            role="menu"
            popover="manual"
            tabIndex={-1}
            data-closing={!menu || undefined}
            // While it fades out it takes no input.
            inert={!menu}
            style={{ positionAnchor: anchor } as CSSProperties}
            // The arrow keys move to the item from the menu itself, which a pointer-opened menu holds.
            onKeyDown={(event) => {
              if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
              event.preventDefault();
              list.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus({ preventScroll: true });
            }}
            aria-label={t("rail.heading.moreLabel", nameLabel)}
          >
            <li role="none">
              <button type="button" role="menuitem" className="rail-menu-item" aria-disabled={saving || undefined} onClick={choose}>
                {project.trusted ? t("rail.heading.distrustButton") : t("rail.heading.trustButton")}
                {/* An unavailable item says why beneath its label (RAIL.md, Menus). */}
                {saving && <span className="rail-menu-reason">{t("rail.heading.savingProgress")}</span>}
              </button>
            </li>
          </ul>
        )}
      </div>
      {asking && onSetTrust && (
        <TrustQuestion
          name={project.name}
          purpose="change"
          takeFocus={initialAsking !== project.id}
          onAnswer={(trusted) => {
            closeQuestion();
            if (trusted) onSetTrust(project, true);
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
        <div id={tilesId} className="rail-group-tiles" inert={folded || undefined}>
          <div className="rail-group-tiles-inner">{children}</div>
        </div>
      )}
    </li>
  );
}
