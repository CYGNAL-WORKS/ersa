import { Archive, Bot, ChevronRight, LoaderCircle } from "lucide-react";
import {
  type AnimationEvent,
  type CSSProperties,
  type FocusEvent,
  type KeyboardEvent,
  type ReactNode,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { t } from "../catalog";
import type { BackendInfo, Session } from "../protocol/core-api.gen";

import { type TileView, tileOf } from "../session/tile";
import { reducedMotion } from "../theme/motion";
import { ContextRing } from "./ContextRing";
import { Icon } from "./Icon";
import { SessionRoot, type TileActions, useTileMenu } from "./TileMenu";
import "./SessionTile.css";

/** Each host's backends, by host id, as `backend.list` gives them. */
export type Backends = Readonly<Record<string, readonly BackendInfo[]>>;

/** The open shelf, as the rail draws it beside its tile. `render` is given the element id the chevron controls. */
export interface OpenShelf {
  id: string;
  tone: "accent" | "caution";
  render: (domId: string) => ReactNode;
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
  fork,
  archive,
  remove,
  initialMenu,
  initialDraft,
  forking,
}: TileActions & {
  session: Session;
  backends?: Backends;
  active: boolean;
  unseen: boolean;
  force?: ForcedTile["state"];
  onSelect: (id: string) => void;
  /** Its shelf is open: the shelf's tone, which the chevron and the want take, and the shelf's element id. */
  shelf?: { tone: "accent" | "caution"; domId: string };
  onChevron?: (id: string) => void;
  /** For the gallery: the tile's menu shown open, and the title being edited with this text. */
  initialMenu?: boolean;
  initialDraft?: string;
  /** A fork of this session is on its way: it says so in place of its state. */
  forking?: boolean;
}) {
  const view = tileOf(session, backends, forking);
  const needsYou = view.mark === "needsYou";
  const { root, editing, rootHandlers, titleField, moreAndMenu } = useTileMenu({
    session,
    title: view.title,
    actions: { rename, pin, fork, archive, remove },
    beforeChevron: needsYou,
    initialMenu,
    initialDraft,
  });
  // A session that comes to need the user: its chevron breathes once where it is (DESIGN.md, Motion: status never
  // moves a tile). Only a change into needing the user does; a tile first drawn needing the user shows it at once.
  const [breath, setBreath] = useState(false);
  const [lastMark, setLastMark] = useState(view.mark);
  const [wasActive, setWasActive] = useState(active);
  // Made active or left mid-breath, the chevron is drawn by another element, which would start the breath over: it ends.
  if (active !== wasActive) {
    setWasActive(active);
    setBreath(false);
  }
  if (view.mark !== lastMark) {
    setLastMark(view.mark);
    // With reduced motion on it never breathes, so nothing is left waiting to breathe should motion come back on.
    setBreath(needsYou && !reducedMotion());
  }
  // Once breathed, it is done. A working glyph's breath loops, so it never ends.
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
      <SessionRoot
        ref={root}
        editing={editing}
        className="session"
        data-force={force}
        data-needs-you={needsYou || undefined}
        aria-current={active || undefined}
        onClick={() => onSelect(session.id)}
        {...rootHandlers}
      >
        <span className="session-backend" role="img" aria-label={view.backend}>
          <Icon icon={Bot} size="icon-sm" />
        </span>
        {editing ? (
          titleField
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
      </SessionRoot>
      {moreAndMenu}
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
      // A slide's transform would take the shelf off its anchor, so the tile that holds one slides by its offset.
      data-slide-top={layer !== undefined || undefined}
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
export function Mark({ view, breath, onAnimationEnd }: { view: TileView; breath?: boolean; onAnimationEnd?: (event: AnimationEvent) => void }) {
  switch (view.mark) {
    case "starting":
      return (
        <span className="session-arc" data-tone={view.tone} aria-hidden="true">
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
