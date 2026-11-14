import { Ellipsis } from "lucide-react";
import { type CSSProperties, type HTMLAttributes, type KeyboardEvent, type MouseEvent, type ReactNode, type Ref, useCallback, useEffect, useId, useRef, useState } from "react";
import { t } from "../catalog";
import type { Session } from "../protocol/core-api.gen";
import { archiveUnavailable } from "../session/archive";
import { deleteUnavailable } from "../session/deletion";
import { forkUnavailable } from "../session/fork";
import type { Sending } from "../session/follow";
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

/** Archive or Unarchive on the menu (FLOW-ARCHIVE-DELETE-SESSION, steps 1 and 3). */
interface TileArchive {
  /** Archive (true) or Unarchive (false), whichever the menu offered for the session as listed. */
  onChoose: (archived: boolean) => void;
  /** Why it can't be used now; given, the menu shows it unavailable with this reason. */
  unavailable?: string;
}

/** Fork on the menu (FLOW-FORK-SESSION, steps 1 and 2). */
interface TileFork {
  /** Starts the fork, which becomes the active session (FLOW-FORK-SESSION-R04). */
  onChoose: () => void;
  /** Why Fork can't be used now; given, the menu shows it unavailable with this reason (R03). */
  unavailable?: string;
}

/** What a session's menu can do. The menu is there only with `rename`. */
export interface TileActions {
  rename?: TileRename;
  pin?: TilePin;
  fork?: TileFork;
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
  /** Pins and unpins on their way, and why the last was refused (R15). */
  pinning?: Readonly<Record<string, Sending>>;
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
  const { onRename, onPin, onFork, onArchive, onDelete, offline = false } = h;
  const offlineReason = offline ? t("rail.tile.offlineReason") : undefined;
  // One pin at a time, so each move in the rail is measured from where the last one left it.
  const saving = Object.values(h.pinning ?? {}).some((p) => p.state === "sending");
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
      unavailable: offlineReason ?? (saving ? t("rail.tile.savingProgress") : undefined),
    },
    fork: onFork && { onChoose: () => onFork(s.id), unavailable: forkUnavailable(s, offline, h.forking?.[s.id]) },
    archive: onArchive && {
      onChoose: (archived) => onArchive(s.id, archived),
      unavailable: archiveUnavailable(s, offline, h.archiving?.[s.id]),
    },
    remove: onDelete && { onAsk: () => onDelete(s.id), unavailable: deleteUnavailable(s, offline) },
  };
}

/**
 * A session's menu, shared by its tile in the rail and its row on the sessions page (LAY-SHELL, A tile's menu): the
 * More button, the menu it opens, right-click and the keys that open it too, and renaming in place. The caller lays
 * out what this returns.
 */
export function useTileMenu({
  session,
  title,
  actions: { rename, pin, fork, archive, remove },
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
  /** For the gallery: the menu shown open, and the title being edited with this text. */
  initialMenu?: boolean;
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
    { open: initialMenu ?? false, place: { anchor }, focus: "none", from: "more" },
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
          {
            kind: "item",
            label: t("rail.tile.renameButton"),
            keyHint: t("rail.tile.renameKey"),
            unavailable: rename.unavailable,
            onChoose: edit,
          },
          ...(fork
            ? [{ kind: "item" as const, label: t("rail.tile.forkButton"), unavailable: fork.unavailable, onChoose: fork.onChoose }]
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
      {rename && <Menu label={label} entries={entries} open={menu.open} place={menu.place} focus={menu.focus} inside={insideMenu} onClose={closeMenu} />}
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
