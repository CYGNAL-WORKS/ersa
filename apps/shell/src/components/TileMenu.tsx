import { Ellipsis } from "lucide-react";
import { type CSSProperties, type HTMLAttributes, type KeyboardEvent, type MouseEvent, type ReactNode, type Ref, useCallback, useEffect, useId, useRef, useState } from "react";
import { t } from "../catalog";
import type { Section, Session } from "../protocol/core-api.gen";
import { archiveUnavailable } from "../session/archive";
import { deleteUnavailable } from "../session/deletion";
import { forkUnavailable } from "../session/fork";
import type { Sending } from "../session/follow";
import { type Moving, type SectionsView, sectionsUnavailable } from "../session/sections";
import { IconButton } from "./Icon";
import { Menu, type MenuClose, type MenuEntry, type MenuPlace } from "./Menu";
import { useTitleDraft } from "./SessionTitle";

/** Renaming a session on its tile or row (FLOW-ORGANIZE-SESSIONS, step 2). */
interface TileRename {
  /** Renames the session through its backend; resolves true once Core has (FLOW-ORGANIZE-SESSIONS-R02). */
  onSave: (title: string) => Promise<boolean>;
  /** A rename on its way, or why the last was refused. */
  renaming?: Sending;
  /** Editing starts or ends: an earlier refusal is no longer the user's concern. */
  onReset: () => void;
  /** Why Rename can't be used now; given, the menu shows it unavailable with this reason, and F2 does nothing. */
  unavailable?: string;
}

/** Delete on the menu (FLOW-ARCHIVE-DELETE-SESSION, step 4): it asks first. */
interface TileDelete {
  /** Opens the delete question. */
  onAsk: () => void;
  /** Why Delete can't be used now; given, the menu shows it unavailable with this reason. */
  unavailable?: string;
}

/** Pinning or unpinning a session (FLOW-ORGANIZE-SESSIONS, step 1). */
interface TilePin {
  /** Whether it is pinned now: the menu offers Unpin if so, else Pin. */
  pinned: boolean;
  /** Pins it when unpinned, unpins it when pinned. */
  onChoose: () => void;
  /** Why Pin or Unpin can't be used now; given, the menu shows it unavailable with this reason. */
  unavailable?: string;
}

/** Move to section ▸ on the menu (FLOW-ORGANIZE-SESSIONS, step 3). */
interface TileMove {
  /** The sections to offer, in the order they were made. */
  sections: readonly Section[];
  /** The section the session is in, which the submenu marks; undefined for none. */
  current?: string;
  /** Moves the session to the section, or to none for null. */
  onChoose: (sectionId: string | null) => void;
  /** Shows a new section's heading, its name field taking focus; once named, the session moves into it. */
  onNew: () => void;
  /** Why it can't be used now; given, the menu shows it unavailable with this reason. */
  unavailable?: string;
}

/** Mark as unread or Mark as read (FLOW-ORGANIZE-SESSIONS-R24). */
interface TileMark {
  /** Whether Core reports the session unseen: the menu offers Mark as read if so, else Mark as unread. */
  unseen: boolean;
  /** Marks it unseen when it isn't, seen when it is. */
  onChoose: () => void;
  /** Why it can't be used now; given, the menu shows it unavailable with this reason. */
  unavailable?: string;
}

/** Archive or Unarchive on the menu (FLOW-ARCHIVE-DELETE-SESSION, steps 1 and 3). */
interface TileArchive {
  /** Archive (true) or Unarchive (false), whichever the menu offered for the session as listed. */
  onChoose: (archived: boolean) => void;
  /** Why it can't be used now; given, the menu shows it unavailable with this reason. */
  unavailable?: string;
}

/** An item that does one thing when chosen: Fork (FLOW-FORK-SESSION, steps 1 and 2) or Copy session ID (FLOW-RESUME-SESSION-R07). */
interface TileItem {
  /** Fork starts the fork, which becomes the active session (FLOW-FORK-SESSION-R04); Copy session ID puts the backend's id on the clipboard. */
  onChoose: () => void;
  /** Why it can't be used now; given, the menu shows it unavailable with this reason. */
  unavailable?: string;
}

/** What a session's menu can do. The menu is there only with `rename`. */
export interface TileActions {
  rename?: TileRename;
  pin?: TilePin;
  mark?: TileMark;
  fork?: TileItem;
  move?: TileMove;
  copyId?: TileItem;
  archive?: TileArchive;
  remove?: TileDelete;
}

/** What the window does for a session's menu, and how those changes stand, by session id. */
export interface TileHandlers {
  /** Renames a session through its backend; resolves true once Core has (FLOW-ORGANIZE-SESSIONS-R02). */
  onRename?: (id: string, title: string) => Promise<boolean>;
  /** Renames on their way, and why the last was refused. */
  renaming?: Readonly<Record<string, Sending>>;
  /** Editing a title starts or ends: its earlier refusal goes. */
  onRenameReset?: (id: string) => void;
  /** Pins or unpins a session (FLOW-ORGANIZE-SESSIONS-R01). */
  onPin?: (id: string, pinned: boolean) => void;
  /** Moves a session to a section, or to none for null (R03). */
  onMove?: (id: string, sectionId: string | null) => void;
  /** Pins, unpins and moves on their way, and why the last was refused (R15). */
  moving?: Readonly<Record<string, Moving>>;
  /** The user's sections, which Move to section offers. */
  sections?: SectionsView;
  /** Shows a new section's heading for the session to move into once it is named (FLOW-ORGANIZE-SESSIONS, step 3). */
  onNewSection?: (id: string) => void;
  /** Marks a session unseen or seen (FLOW-ORGANIZE-SESSIONS-R24). */
  onMark?: (id: string, unseen: boolean) => void;
  /** Marks on their way, and why the last was refused. */
  marking?: Readonly<Record<string, Sending>>;
  /** The active session, which can't be marked unseen: the user is looking at it. */
  activeId?: string;
  /** Forks a session (FLOW-FORK-SESSION); the fork becomes the active session once Core has started it. */
  onFork?: (id: string) => void;
  /** Forks on their way, and why the last was refused, by the original's id. */
  forking?: Readonly<Record<string, Sending>>;
  /** Archives a session, or unarchives an archived one (FLOW-ARCHIVE-DELETE-SESSION, steps 1 and 3). */
  onArchive?: (id: string, archived: boolean) => void;
  /** Archives and unarchives on their way, and why the last was refused. */
  archiving?: Readonly<Record<string, Sending>>;
  /** Asks before deleting a session (FLOW-ARCHIVE-DELETE-SESSION, step 4). */
  onDelete?: (id: string) => void;
  /** Ersa can't reach Core, so nothing can be renamed, pinned, archived or deleted now. */
  offline?: boolean;
}

/** A session's menu actions, each unavailable with its reason while it can't be used. */
export function tileActions(s: Session, h: TileHandlers): TileActions {
  const { onRename, onPin, onMark, onMove, onNewSection, onFork, onArchive, onDelete, offline = false } = h;
  const offlineReason = offline ? t("rail.tile.offlineReason") : undefined;
  // One pin or move at a time, so each move in the rail is measured from where the last one left it.
  const saving = Object.values(h.moving ?? {}).some((p) => p.state === "sending");
  const savingReason = saving ? t("rail.tile.savingProgress") : undefined;
  return {
    // Unavailable only while Ersa can't reach Core. A session its backend has not named yet, such as one still
    // starting, is refused by Core, and the field says why.
    rename: onRename && {
      onSave: (title) => onRename(s.id, title),
      renaming: h.renaming?.[s.id],
      onReset: () => h.onRenameReset?.(s.id),
      unavailable: offlineReason,
    },
    pin: onPin && {
      pinned: s.pinned,
      onChoose: () => onPin(s.id, !s.pinned),
      unavailable: offlineReason ?? savingReason,
    },
    mark: onMark && {
      unseen: s.unseen,
      onChoose: () => onMark(s.id, !s.unseen),
      unavailable: markUnavailable(s, h, offlineReason),
    },
    move: onMove &&
      onNewSection && {
        sections: h.sections?.state === "ready" ? h.sections.sections : [],
        current: s.section_id,
        onChoose: (sectionId) => onMove(s.id, sectionId),
        onNew: () => onNewSection(s.id),
        unavailable: offlineReason ?? savingReason ?? sectionsUnavailable(h.sections),
      },
    fork: onFork && { onChoose: () => onFork(s.id), unavailable: forkUnavailable(s, offline, h.forking?.[s.id]) },
    copyId: copyIdAction(s),
    archive: onArchive && {
      onChoose: (archived) => onArchive(s.id, archived),
      unavailable: archiveUnavailable(s, offline, h.archiving?.[s.id]),
    },
    remove: onDelete && { onAsk: () => onDelete(s.id), unavailable: deleteUnavailable(s, offline) },
  };
}

// Copy session ID asks Core nothing, so it stays available while Ersa can't reach Core. Before the backend has
// reported its id for the session, Core lists none, and the item says so.
function copyIdAction(s: Session): TileItem {
  const id = s.backend_session_id;
  if (id === undefined) return { onChoose: () => {}, unavailable: t("rail.tile.copySessionIdReason") };
  return {
    // A write the webview refuses leaves the clipboard as it was; the menu has closed either way, so there is nowhere
    // to say so, and the user can choose it again.
    onChoose: () => void navigator.clipboard.writeText(id).catch(() => {}),
  };
}

// Mark as unread is unavailable on the active session, which the user is looking at (FLOW-ORGANIZE-SESSIONS-R24);
// Mark as read never is, so a mark Core still reports there can always go.
function markUnavailable(s: Session, h: TileHandlers, offlineReason: string | undefined): string | undefined {
  if (offlineReason) return offlineReason;
  if (h.marking?.[s.id]?.state === "sending") return t("rail.tile.savingProgress");
  if (!s.unseen && s.id === h.activeId) return t("rail.tile.markUnreadActiveReason");
  return undefined;
}

/**
 * A session's menu, shared by its tile in the rail and its row on the sessions page (LAY-SHELL, A tile's menu): the
 * More button, the menu it opens, right-click and the keys that open it too, and renaming in place. The caller lays
 * out what this returns.
 */
export function useTileMenu({
  session,
  title,
  actions: { rename, pin, mark, fork, move, copyId, archive, remove },
  beforeChevron,
  initialMenu,
  initialDraft,
}: {
  session: Session;
  /** The title as shown, which names the menu. */
  title: string;
  actions: TileActions;
  /** The tile's More button leaves room for the chevron beside it. */
  beforeChevron?: boolean;
  /** For the gallery: the menu shown open, "move" with Move to section's submenu open too, and the title being edited with this text. */
  initialMenu?: boolean | "move";
  initialDraft?: string;
}) {
  const root = useRef<HTMLElement>(null);
  const more = useRef<HTMLButtonElement>(null);
  const field = useRef<HTMLInputElement>(null);
  // The More button names the anchor the menu opens below.
  const anchor = `--tile-more-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  // The menu: whether it is open, where it sits, where focus went, and what opened it, which takes focus back.
  // Kept as it closes, so the menu fades out where it was.
  const [menu, setMenu] = useState<{ open: boolean; place: MenuPlace; focus: "item" | "menu" | "none"; from: "more" | "tile" }>(
    { open: !!initialMenu, place: { anchor }, focus: "none", from: "more" },
  );
  const opener = useRef(menu.from);
  opener.current = menu.from;
  const closeMenu = useCallback((how: MenuClose) => {
    setMenu((was) => ({ ...was, open: false }));
    // Closing gives focus back to what opened it, unless the user put focus somewhere else.
    if (how !== "away") (opener.current === "more" ? more.current : root.current)?.focus({ preventScroll: true });
  }, []);
  const insideMenu = useCallback(() => [more.current], []);

  // The title being edited in place of the title (useTitleDraft, shared with the header).
  const { draft, setDraft, busy, refused, edit, onFieldKey } = useTitleDraft({
    title: session.title ?? "",
    untitled: !session.title,
    onRename: rename?.onSave,
    renaming: rename?.renaming,
    onReset: rename?.onReset,
    disabled: rename?.unavailable !== undefined,
    initialDraft,
    field,
    home: root,
  });
  // The field takes focus as it opens, its text selected to type over.
  const editing = draft !== undefined;
  useEffect(() => {
    if (!editing || initialDraft !== undefined) return;
    field.current?.focus();
    field.current?.select();
  }, [editing, initialDraft]);

  const beingDeleted = session.deleting !== undefined;
  const deleteItem: MenuEntry | undefined = remove && {
    kind: "item",
    label: t("rail.tile.deleteButton"),
    danger: true,
    unavailable: remove.unavailable,
    onChoose: remove.onAsk,
  };
  const entries: MenuEntry[] = !rename
    ? []
    : // A session being deleted, or whose deletion failed, holds Delete… alone (LAY-SHELL, Being deleted).
      beingDeleted && deleteItem
      ? [deleteItem]
      : [
          ...(pin
            ? [
                {
                  kind: "item" as const,
                  label: pin.pinned ? t("rail.tile.unpinButton") : t("rail.tile.pinButton"),
                  unavailable: pin.unavailable,
                  onChoose: pin.onChoose,
                },
              ]
            : []),
          ...(mark
            ? [
                {
                  kind: "item" as const,
                  label: mark.unseen ? t("rail.tile.markReadButton") : t("rail.tile.markUnreadButton"),
                  unavailable: mark.unavailable,
                  onChoose: mark.onChoose,
                },
              ]
            : []),
          {
            kind: "item",
            label: t("rail.tile.renameButton"),
            keyHint: t("rail.tile.renameKey"),
            unavailable: rename.unavailable,
            onChoose: edit,
          },
          ...(fork
            ? [plainItem(t("rail.tile.forkButton"), fork)]
            : []),
          ...(move ? [moveEntry(move)] : []),
          ...(copyId
            ? [plainItem(t("rail.tile.copySessionIdButton"), copyId)]
            : []),
          ...(archive
            ? [
                {
                  kind: "item" as const,
                  label: session.archived ? t("rail.tile.unarchiveButton") : t("rail.tile.archiveButton"),
                  unavailable: archive.unavailable,
                  onChoose: () => archive.onChoose(!session.archived),
                },
              ]
            : []),
          // Delete stands apart in its own group, its label in danger (docs/design/RAIL.md, Menus).
          ...(deleteItem ? [{ kind: "rule" } as const, deleteItem] : []),
        ];

  const rootHandlers = {
    onContextMenu: (event: MouseEvent) => {
      if (!rename) return;
      // A right-click opens the menu at the pointer. The context-menu key and Shift+F10 may send this event too,
      // after their own keydown has opened the menu at the tile: that one stays.
      event.preventDefault();
      setMenu((was) =>
        was.open && was.focus === "item" && was.from === "tile" ? was : { open: true, place: { x: event.clientX, y: event.clientY }, focus: "menu", from: "tile" },
      );
    },
    onKeyDown: (event: KeyboardEvent) => {
      if (!rename) return;
      if (event.key === "F2" && !beingDeleted) {
        event.preventDefault();
        edit();
      } else if (event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey)) {
        // The context-menu key and Shift+F10 open the same menu, at the tile, with focus on its first item.
        event.preventDefault();
        setMenu({ open: true, place: { anchor }, focus: "item", from: "tile" });
      }
    },
  };

  const titleField: ReactNode = editing && (
    <input
      ref={field}
      className="session-title-field"
      aria-label={t("rail.tile.titleLabel")}
      value={draft}
      readOnly={busy}
      aria-invalid={refused !== undefined || undefined}
      aria-errormessage={refused !== undefined ? `${anchor}-refused` : undefined}
      onChange={(event) => {
        setDraft(event.target.value);
        // Typing again leaves an earlier refusal behind.
        if (refused !== undefined) rename?.onReset();
      }}
      // Enter saves and Escape restores the title as it was (FLOW-ORGANIZE-SESSIONS, step 2), as in the header.
      onKeyDown={(event) => onFieldKey(event, { enter: true })}
      // Leaving the field without Enter restores the title too, unless a rename is on its way or was refused.
      onBlur={() => {
        if (!busy && refused === undefined) setDraft(undefined);
      }}
    />
  );

  const label = t("rail.tile.moreLabel", { title });
  const moreAndMenu: ReactNode = (
    <>
      {rename && !editing && (
        <IconButton
          ref={more}
          className="session-more"
          icon={Ellipsis}
          size="icon-sm"
          label={label}
          aria-haspopup="menu"
          aria-expanded={menu.open}
          data-before-chevron={beforeChevron || undefined}
          style={{ anchorName: anchor } as CSSProperties}
          // A click the keyboard made (Enter or Space) has no pointer presses to count.
          onClick={(event) =>
            setMenu(menu.open ? { ...menu, open: false } : { open: true, place: { anchor }, focus: event.detail === 0 ? "item" : "menu", from: "more" })
          }
        />
      )}
      {rename && (
        <Menu
          label={label}
          entries={entries}
          open={menu.open}
          place={menu.place}
          focus={menu.focus}
          inside={insideMenu}
          onClose={closeMenu}
          initialSubmenu={initialMenu === "move" ? entries.findIndex((e) => e.kind === "submenu") : undefined}
        />
      )}
      {/* A refused title keeps the field open, with the reason beneath. */}
      {editing && refused !== undefined && (
        <p id={`${anchor}-refused`} className="problem session-rename-problem" role="alert">
          {refused}
        </p>
      )}
    </>
  );

  return { root, editing, rootHandlers, titleField, moreAndMenu };
}

/** A menu row that does one thing when chosen, unavailable with its reason while it can't. */
function plainItem(label: string, item: TileItem): MenuEntry {
  return { kind: "item", label, unavailable: item.unavailable, onChoose: item.onChoose };
}

/**
 * Move to section ▸ (LAY-SHELL, A tile's menu): each section, the current one marked, then "No section", marked when
 * the session is in none, and, after a rule, "New section…" (docs/design/RAIL.md, Menus). Choosing where the session
 * already is changes nothing, so it asks Core nothing.
 */
function moveEntry(move: TileMove): MenuEntry {
  const to = (sectionId: string | null) => () => {
    if (sectionId !== (move.current ?? null)) move.onChoose(sectionId);
  };
  return {
    kind: "submenu",
    label: t("rail.tile.moveToSectionButton"),
    unavailable: move.unavailable,
    entries: [
      ...move.sections.map((section): MenuEntry => ({ kind: "item", label: section.name, checked: section.id === move.current, onChoose: to(section.id) })),
      { kind: "item", label: t("rail.tile.noSectionButton"), checked: move.current === undefined, onChoose: to(null) },
      { kind: "rule" },
      { kind: "item", label: t("rail.tile.newSectionButton"), onChoose: move.onNew },
    ],
  };
}

/**
 * The root of a tile or row: the button that makes its session active, or, while its title is being edited, a box of
 * the same look holding the field, since a field can't sit inside a button.
 */
export function SessionRoot({
  editing,
  ref,
  ...rest
}: HTMLAttributes<HTMLElement> & { editing: boolean; ref: Ref<HTMLElement>; "data-force"?: string; "data-needs-you"?: boolean }) {
  if (editing) {
    // The box only holds the field: choosing, the menu and its keys belong to the tile when it is a button again.
    const { onClick: _click, onContextMenu: _menu, onKeyDown: _key, ...box } = rest;
    return <div ref={ref as Ref<HTMLDivElement>} data-editing {...box} />;
  }
  return <button ref={ref as Ref<HTMLButtonElement>} type="button" {...rest} />;
}
