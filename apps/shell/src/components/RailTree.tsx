import { type FocusEvent, type KeyboardEvent, type ReactNode, useEffect, useRef, useState } from "react";
import { ChevronDown, Ellipsis, Plus, ShieldAlert } from "lucide-react";
import { t } from "../catalog";
import type { ProjectsView, SessionsView } from "../core/connection";
import type { Project, Session, Workspace } from "../protocol/core-api.gen";
import type { Sending } from "../session/follow";
import { treeOf } from "../session/tree";
import { Icon, IconButton } from "./Icon";
import { type Backends, type ForcedTile, SessionTile } from "./SessionTile";
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
  /** For the gallery: a pointer or keyboard state on one tile or one heading, the menu open, the trust question asked. */
  force?: ForcedTile;
  forceHeading?: { id: string; state: "hover" | "focus" | "pressed" };
  initialMenu?: string;
  initialAsking?: string;
  /** Shown beneath the tree: the way to add a project. */
  children?: ReactNode;
}

/** The rail's tree (LAY-SHELL, The rail): each project's heading with its sessions beneath it, and their notes. */
export function RailTree(props: RailTreeProps) {
  const { projects, sessions, workspaces, placing, activeId, onSelect, backends, unseen, force, children } = props;
  const listed = projects.state === "ready" ? projects.projects : [];
  const tree = treeOf(listed, workspaces, sessions.state === "ready" ? sessions.sessions : []);
  // A session waits to be placed while the projects or their workspaces are still being read, so it does not
  // sit at the end and then move. When the projects could not be read, every session sits at the end.
  const looseShown = projects.state === "error" || (projects.state === "ready" && !placing);
  const tile = (s: Session) => (
    <li key={s.id}>
      <SessionTile
        session={s}
        backends={backends}
        active={s.id === activeId}
        unseen={unseen?.has(s.id) ?? false}
        force={force?.id === s.id ? force.state : undefined}
        onSelect={onSelect}
      />
    </li>
  );
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
          {tree.projects.map(({ project, sessions }) => (
            <ProjectGroup key={project.id} project={project} {...props}>
              {sessions.length > 0 && <ul className="rail-tiles">{sessions.map(tile)}</ul>}
            </ProjectGroup>
          ))}
        </ul>
      )}
      {looseShown && tree.loose.length > 0 && <ul className="rail-tiles">{tree.loose.map(tile)}</ul>}
      {children}
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
  children,
}: RailTreeProps & { project: Project; children: ReactNode }) {
  const [menu, setMenu] = useState(initialMenu === project.id);
  const [asking, setAsking] = useState(initialAsking === project.id);
  const group = useRef<HTMLLIElement>(null);
  const more = useRef<HTMLButtonElement>(null);
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
    <li ref={group} className="rail-project">
      <div className="rail-heading" data-force={forced} data-open={menu || undefined} onKeyDown={onHeadingKey} onBlur={onHeadingBlur}>
        <h3 className="rail-heading-title">
          {/* Collapsing arrives with its own ticket, and with it aria-expanded; until then every project is expanded. */}
          <button type="button" className="rail-heading-toggle">
            <span className="rail-heading-chevron">
              <Icon icon={ChevronDown} size="icon-sm" />
            </span>
            <span className="rail-heading-name">{project.name}</span>
            {host !== undefined && <span className="rail-heading-host">{host}</span>}
            {!project.trusted && (
              <span className="rail-heading-trust">
                <Icon icon={ShieldAlert} size="icon-sm" />
                {t("rail.heading.notTrustedStatus")}
              </span>
            )}
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
              onClick={(event) => {
                // A click the keyboard made (Enter or Space) has no pointer presses to count.
                focusOnOpen.current = menu ? undefined : event.detail === 0 ? "item" : "menu";
                setMenu(!menu);
              }}
            />
          )}
        </span>
        {menu && onSetTrust && (
          <ul
            ref={list}
            className="rail-menu"
            role="menu"
            tabIndex={-1}
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
      {children}
    </li>
  );
}
