import { Archive, Bot, ChevronRight, Ellipsis, LoaderCircle } from "lucide-react";
import {
  type AnimationEvent,
  type CSSProperties,
  type FocusEvent,
  type HTMLAttributes,
  type KeyboardEvent,
  type ReactNode,
  type Ref,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { t } from "../catalog";
import type { Session } from "../protocol/core-api.gen";
import type { BackendChoices } from "../session/controls";
import { type TileView, tileOf } from "../session/tile";
import type { Sending } from "../session/follow";
import { reducedMotion } from "../theme/motion";
import { ContextRing } from "./ContextRing";
import { Icon, IconButton } from "./Icon";
import { Menu, type MenuClose, type MenuPlace } from "./Menu";
import { useTitleDraft } from "./SessionTitle";
import "./SessionTile.css";

/** Each host's backends, by host id, as `backend.list` gives them. */
export type Backends = Readonly<Record<string, readonly BackendChoices[]>>;

/** The open shelf, as the rail draws it beside its tile. `render` is given the element id the chevron controls. */
export interface OpenShelf {
  id: string;
  tone: "accent" | "caution";
  render: (domId: string) => ReactNode;
}

/** Renaming a session on its tile (FLOW-ORGANIZE-SESSIONS, step 2). */
export interface TileRename {
  /** Renames the session through its backend; resolves true once Core has (FLOW-ORGANIZE-SESSIONS-R02). */
  onSave: (title: string) => Promise<boolean>;
  /** A rename on its way, or why the last was refused. */
  renaming?: Sending;
  /** Editing starts or ends: an earlier refusal is no longer the user's concern. */
  onReset: () => void;
  /** Why Rename can't be used now; given, the menu shows it unavailable with this reason, and F2 does nothing. */
  unavailable?: string;
}

/** Pinning or unpinning a session from its tile (FLOW-ORGANIZE-SESSIONS, step 1). */
export interface TilePin {
  /** Whether it is pinned now: the menu offers Unpin if so, else Pin. */
  pinned: boolean;
  /** Pins it when unpinned, unpins it when pinned. */
  onChoose: () => void;
  /** Why Pin or Unpin can't be used now; given, the menu shows it unavailable with this reason. */
  unavailable?: string;
}

/** Archive or Unarchive on the tile's menu (FLOW-ARCHIVE-DELETE-SESSION, steps 1 and 3). */
export interface TileArchive {
  /** Archive (true) or Unarchive (false), whichever the menu offered for the session as listed. */
  onChoose: (archived: boolean) => void;
  /** Why it can't be used now; given, the menu shows it unavailable with this reason. */
  unavailable?: string;
}

/** For the gallery: show a pointer or keyboard state on one tile without one. */
export type ForcedTile = { id: string; state: "hover" | "focus" | "pressed" };

/**
 * A session's tile (docs/design/RAIL.md, A tile): two lines with no box. Line one is the backend's icon, the title
 * and the state's mark; line two the context ring, under the icon, then a phrase and the cost. `tileOf` decides
 * what each shows; this only lays it out. The mark has the trailing edge to itself, one square on line one.
 */
export function SessionTile({
  session,
  backends,
  active,
  unseen,
  force,
  onSelect,
  shelf,
  onChevron,
  rename,
  pin,
  archive,
  initialMenu,
  initialDraft,
}: {
  session: Session;
  backends?: Backends;
  active: boolean;
  unseen: boolean;
  force?: ForcedTile["state"];
  onSelect: (id: string) => void;
  /** Its shelf is open: the shelf's tone, which the chevron and the want take, and the shelf's element id. */
  shelf?: { tone: "accent" | "caution"; domId: string };
  onChevron?: (id: string) => void;
  /** Rename on the tile's menu and F2; absent, the tile has no menu. */
  rename?: TileRename;
  /** Pin or Unpin, first on the tile's menu (LAY-SHELL, A tile's menu). */
  pin?: TilePin;
  /** Archive or Unarchive on the tile's menu, after Rename; the menu is there only with Rename. */
  archive?: TileArchive;
  /** For the gallery: the tile's menu shown open, and the title being edited with this text. */
  initialMenu?: boolean;
  initialDraft?: string;
}) {
  const view = tileOf(session, backends);
  const tile = useRef<HTMLElement>(null);
  const more = useRef<HTMLButtonElement>(null);
  const field = useRef<HTMLInputElement>(null);
  // The More button names the anchor the menu opens below.
  const anchor = `--tile-more-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  // The tile's menu: whether it is open, where it sits, where focus went, and what opened it, which takes focus back.
  // Kept as it closes, so the menu fades out where it was.
  const [menu, setMenu] = useState<{ open: boolean; place: MenuPlace; focus: "item" | "menu" | "none"; from: "more" | "tile" }>(
    { open: initialMenu ?? false, place: { anchor }, focus: "none", from: "more" },
  );
  const opener = useRef(menu.from);
  opener.current = menu.from;
  const closeMenu = useCallback((how: MenuClose) => {
    setMenu((was) => ({ ...was, open: false }));
    // Closing gives focus back to what opened it, unless the user put focus somewhere else.
    if (how !== "away") (opener.current === "more" ? more.current : tile.current)?.focus({ preventScroll: true });
  }, []);
  const insideMenu = useCallback(() => [more.current], []);

  // The title being edited on the tile, in place of the title; undefined while it is only shown.
  // The title being edited on the tile, in place of the title (useTitleDraft, shared with the header).
  const { draft, setDraft, busy, refused, edit, onFieldKey } = useTitleDraft({
    title: session.title ?? "",
    untitled: !session.title,
    onRename: rename?.onSave,
    renaming: rename?.renaming,
    onReset: rename?.onReset,
    disabled: rename?.unavailable !== undefined,
    initialDraft,
    field,
    home: tile,
  });
  // The field takes focus as it opens, its text selected to type over.
  const editing = draft !== undefined;
  useEffect(() => {
    if (!editing || initialDraft !== undefined) return;
    field.current?.focus();
    field.current?.select();
  }, [editing, initialDraft]);
  const needsYou = view.mark === "needsYou";
  // A session that comes to need the user: its chevron breathes once where it is (DESIGN.md, Motion: status never
  // moves a tile). Only a change into needing the user does; a tile first drawn needing the user shows it at once.
  const [breath, setBreath] = useState(false);
  const [lastMark, setLastMark] = useState(view.mark);
  if (view.mark !== lastMark) {
    setLastMark(view.mark);
    // With reduced motion on it never breathes, so nothing is left waiting to breathe should motion come back on.
    setBreath(needsYou && !reducedMotion());
  }
  // Once breathed, it is done: a chevron drawn again, as the active tile's mark or its shelf's handle, holds still. A
  // working glyph's breath loops, so it never ends.
  const breathed = (event: AnimationEvent) => {
    if (event.animationName === "session-breath") setBreath(false);
  };
  // The chevron is the shelf's handle on every tile but the active one, whose requests the transcript answers
  // (FLOW-APPROVE-PERMISSION-R26); there it stays a mark that names the want.
  const handle = needsYou && !active;
  const tone = shelf?.tone ?? view.tone;
  return (
    <>
      {/* The tile makes its session active; the chevron, laid over its trailing square, opens its shelf. */}
      <Root
        ref={tile}
        editing={editing}
        className="session"
        data-force={force}
        data-needs-you={needsYou || undefined}
        aria-current={active || undefined}
        onClick={() => onSelect(session.id)}
        onContextMenu={(event) => {
          if (!rename) return;
          // A right-click opens the tile's menu at the pointer. The context-menu key and Shift+F10 may send this
          // event too, after their own keydown has opened the menu at the tile: that one stays.
          event.preventDefault();
          setMenu((was) =>
            was.open && was.focus === "item" && was.from === "tile"
              ? was
              : { open: true, place: { x: event.clientX, y: event.clientY }, focus: "menu", from: "tile" },
          );
        }}
        onKeyDown={(event) => {
          if (!rename) return;
          if (event.key === "F2") {
            event.preventDefault();
            edit();
          } else if (event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey)) {
            // The context-menu key and Shift+F10 open the same menu, at the tile, with focus on its first item.
            event.preventDefault();
            setMenu({ open: true, place: { anchor }, focus: "item", from: "tile" });
          }
        }}
      >
        <span className="session-backend" role="img" aria-label={view.backend}>
          <Icon icon={Bot} size="icon-sm" />
        </span>
        {editing ? (
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
        ) : (
          <span className="session-title" data-muted={(view.titleMuted && !unseen) || undefined} data-unseen={unseen || undefined}>
            {view.title}
          </span>
        )}
        <span className="session-mark">{!handle && <Mark view={view} breath={breath} onAnimationEnd={breathed} />}</span>
        {view.ring !== undefined && (
          <span className="session-ring">
            <ContextRing used={view.ring} size="ring-sm" />
          </span>
        )}
        <span className="session-detail">
          {/* On the active tile a want is named by its chevron mark, so it isn't read twice. */}
          {view.phrase && (
            <span className="session-phrase" data-tone={needsYou ? tone : undefined} aria-hidden={(needsYou && !handle) || undefined}>
              {view.phrase}
            </span>
          )}
          {view.phrase && view.cost && " · "}
          {view.cost}
        </span>
      </Root>
      {rename && !editing && (
        <IconButton
          ref={more}
          className="session-more"
          icon={Ellipsis}
          size="icon-sm"
          label={t("rail.tile.moreLabel", { title: view.title })}
          aria-haspopup="menu"
          aria-expanded={menu.open}
          data-before-chevron={needsYou || undefined}
          style={{ anchorName: anchor } as CSSProperties}
          // A click the keyboard made (Enter or Space) has no pointer presses to count.
          onClick={(event) =>
            setMenu(menu.open ? { ...menu, open: false } : { open: true, place: { anchor }, focus: event.detail === 0 ? "item" : "menu", from: "more" })
          }
        />
      )}
      {rename && (
        <Menu
          label={t("rail.tile.moreLabel", { title: view.title })}
          entries={[
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
          ]}
          open={menu.open}
          place={menu.place}
          focus={menu.focus}
          inside={insideMenu}
          onClose={closeMenu}
        />
      )}
      {/* A refused title keeps the field open, with the reason beneath the tile. */}
      {editing && refused !== undefined && (
        <p id={`${anchor}-refused`} className="problem session-rename-problem" role="alert">
          {refused}
        </p>
      )}
      {handle && (
        <button
          type="button"
          className="session-chevron-button"
          data-tone={tone}
          data-breath={breath ? "once" : undefined}
          onAnimationEnd={breathed}
          aria-label={t("rail.shelf.nameLabel", { title: view.title })}
          aria-haspopup="dialog"
          aria-expanded={shelf !== undefined}
          aria-controls={shelf?.domId}
          onClick={() => onChevron?.(session.id)}
        >
          <Icon icon={ChevronRight} size="icon-sm" />
        </button>
      )}
    </>
  );
}

/**
 * A tile's place in the list and, while its shelf is open, the shelf beside it. The open tile names the anchor the
 * shelf is placed by, and tells the shelf where the two join, so they read as one shape (docs/design/RAIL.md, The
 * shelf: One shape).
 */
export function TileItem({
  id,
  shelf,
  onClose: close,
  children,
}: {
  id: string;
  shelf?: OpenShelf;
  onClose?: (id: string) => void;
  children: (shelfDomId: string) => ReactNode;
}) {
  const onClose = () => close?.(id);
  const item = useRef<HTMLLIElement>(null);
  const domId = useId();
  const open = shelf !== undefined;
  const shown = shelf?.render(domId);
  // The shelf last drawn, kept while it slides back after closing (docs/design/RAIL.md, The shelf: Motion).
  const last = useRef<ReactNode>(undefined);
  // Kept once committed, so a render React discards never slides back.
  useLayoutEffect(() => {
    if (shown !== undefined) last.current = shown;
  });
  // Decided in the render that closes it, so the layer never leaves the page for a frame and the shelf keeps its
  // element, and the join measured on it, as it slides back.
  const [closing, setClosing] = useState<ReactNode>();
  const [wasOpen, setWasOpen] = useState(open);
  if (open !== wasOpen) {
    setWasOpen(open);
    // With reduced motion on, it goes at once.
    setClosing(open || reducedMotion() ? undefined : last.current);
  }
  const closed = (event: AnimationEvent) => {
    if (event.animationName === "shelf-back") setClosing(undefined);
  };
  useLayoutEffect(() => {
    const li = item.current;
    const layer = li?.querySelector<HTMLElement>(".shelf");
    if (!li || !layer) return;
    // Measured, not designed: where the tile meets the shelf, which moves as the rail scrolls or the shelf rises.
    const place = () => {
      const tile = li.getBoundingClientRect();
      const box = layer.getBoundingClientRect();
      const top = tile.top - box.top;
      layer.style.setProperty("--shelf-join-top", `${top}px`);
      layer.style.setProperty("--shelf-join-height", `${tile.height}px`);
      layer.toggleAttribute("data-risen", top > 0.5);
    };
    place();
    const observer = new ResizeObserver(place);
    observer.observe(li);
    observer.observe(layer);
    window.addEventListener("resize", place);
    document.addEventListener("scroll", place, true);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", place);
      document.removeEventListener("scroll", place, true);
    };
  }, [open]);
  // Whether keyboard focus is in the shelf, kept as focus moves, since a shelf that goes at once takes its focus
  // with it. Focus that goes nowhere (the window loses it, or the element holding it goes) leaves this as it was.
  const focusInShelf = useRef(false);
  const inShelf = (target: EventTarget | null) => target instanceof Node && (item.current?.querySelector(".shelf-layer")?.contains(target) ?? false);
  // Opening moves focus to the shelf itself, not to an answer (FLOW-APPROVE-PERMISSION-R05). Closing with focus in it
  // puts focus on the chevron, or on the tile once nothing is pending and the chevron is gone (R06, R08).
  useLayoutEffect(() => {
    const li = item.current;
    if (!li) return;
    if (open) {
      li.querySelector<HTMLElement>(".shelf-layer:not([inert]) > .shelf")?.focus();
      return;
    }
    if (!focusInShelf.current) return;
    focusInShelf.current = false;
    (li.querySelector<HTMLElement>(".session-chevron-button") ?? li.querySelector<HTMLElement>(".session"))?.focus();
  }, [open]);
  // A choice outside the shelf and its tile closes it, and leaves focus where the user put it (R07).
  useEffect(() => {
    if (!open) return;
    const away = (event: PointerEvent) => {
      if (!(event.target instanceof Node) || item.current?.contains(event.target)) return;
      // Focus is the user's to place, or to leave, with this press: closing gives none back (R07). A press that takes
      // no focus, such as on a scrollbar, leaves it to fall from the shelf as it goes.
      focusInShelf.current = false;
      close?.(id);
    };
    document.addEventListener("pointerdown", away, true);
    return () => document.removeEventListener("pointerdown", away, true);
  }, [open, id, close]);
  const focusIn = (event: FocusEvent) => {
    focusInShelf.current = inShelf(event.target);
  };
  const focusOut = (event: FocusEvent) => {
    const to = event.relatedTarget;
    if (to === null) return;
    focusInShelf.current = inShelf(to);
    // Focus left the shelf and its tile for somewhere else in the window (R07).
    if (open && to instanceof Node && !item.current?.contains(to)) onClose();
  };
  // Escape in the shelf or on its chevron closes it (R06).
  const keyDown = (event: KeyboardEvent) => {
    if (!open || event.key !== "Escape") return;
    const target = event.target as Element;
    if (!inShelf(target) && !target.classList.contains("session-chevron-button")) return;
    event.preventDefault();
    focusInShelf.current = true;
    onClose();
  };
  const layer = shown ?? closing;
  // Each tile names its own anchor, so a shelf sliding back stays by its tile while another tile's opens.
  const anchor = `--tile-${domId.replace(/[^\w-]/g, "")}`;
  return (
    <li
      ref={item}
      className="session-item"
      data-slide={`session:${id}`}
      data-shelf={open || undefined}
      data-tone={shelf?.tone}
      style={layer !== undefined ? ({ anchorName: anchor } as CSSProperties) : undefined}
      // The rail finds a tile by its session, to move focus when one leaves.
      data-session-id={id}
      onFocus={focusIn}
      onBlur={focusOut}
      onKeyDown={keyDown}
    >
      {children(domId)}
      {/* One layer, open or closing, so the shelf keeps its element, and its measured join, as it slides back. */}
      {layer !== undefined && (
        <div className="shelf-layer" inert={!open || undefined} onAnimationEnd={closed} style={{ positionAnchor: anchor } as CSSProperties}>
          {layer}
        </div>
      )}
    </li>
  );
}

/** The state's mark, named where line two doesn't already name it. */
function Mark({ view, breath, onAnimationEnd }: { view: TileView; breath: boolean; onAnimationEnd: (event: AnimationEvent) => void }) {
  switch (view.mark) {
    case "starting":
      return (
        <span className="session-arc" aria-hidden="true">
          <Icon icon={LoaderCircle} size="icon-xs" />
        </span>
      );
    case "working":
      // It breathes for as long as the session works (DESIGN.md, Motion: a working session's glyph).
      return <span className="glyph session-glyph" data-tone={view.tone} data-breath="loop" role="img" aria-label={view.markLabel} />;
    case "needsYou":
      // The active tile's: a mark, not a control, since the transcript answers its requests.
      return (
        <span className="session-chevron" data-tone={view.tone} data-breath={breath ? "once" : undefined} role="img" aria-label={view.markLabel} onAnimationEnd={onAnimationEnd}>
          <Icon icon={ChevronRight} size="icon-sm" />
        </span>
      );
    // Line two names these, so their marks are hidden from assistive technology.
    case "disconnected":
    case "error":
      return <span className="glyph session-glyph" data-tone={view.tone} aria-hidden="true" />;
    // Line two says the state; the archive mark is named, since nothing else says the session is archived.
    case "archived":
      return (
        <span className="session-archived" role="img" aria-label={view.markLabel}>
          <Icon icon={Archive} size="icon-xs" />
        </span>
      );
    default:
      return null;
  }
}

/**
 * The tile's root: the button that makes its session active, or, while its title is being edited, a box of the same
 * look holding the field, since a field can't sit inside a button.
 */
function Root({
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
