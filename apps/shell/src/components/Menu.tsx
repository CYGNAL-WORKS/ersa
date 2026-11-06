import { Check, ChevronRight } from "lucide-react";
import { type CSSProperties, type KeyboardEvent, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { Icon } from "./Icon";
import "./Menu.css";

/** One row of a menu (docs/design/RAIL.md, Menus). */
export type MenuEntry =
  | {
      kind: "item";
      label: string;
      /** Chosen: the menu and every menu above it close first. */
      onChoose?: () => void;
      /** Why the item can't be used now. Given, the item is unavailable and says this beneath its label. */
      unavailable?: string;
      /** Delete and its kind: the label reads in the danger tone. */
      danger?: boolean;
      /** A key that does the same, at the item's trailing edge. */
      keyHint?: string;
      /** In a submenu: the chosen option, marked with a check. */
      checked?: boolean;
    }
  | {
      kind: "submenu";
      label: string;
      /** The current choice, before the chevron (the arrange menu's). */
      current?: string;
      entries: MenuEntry[];
      unavailable?: string;
    }
  | { kind: "rule" };

/** How a menu closed: the owner returns focus to what opened it for each but "away". */
export type MenuClose = "escape" | "choose" | "away";

/** Where a menu sits: below an anchor, aligned to its trailing edge; or at the pointer. */
export type MenuPlace = { anchor: string } | { x: number; y: number };

interface MenuProps {
  /** The menu's name for assistive technology. */
  label: string;
  entries: MenuEntry[];
  /** Open, or fading out once closed. The owner keeps the menu mounted; it draws nothing while closed. */
  open: boolean;
  place: MenuPlace;
  /**
   * Where focus goes as it opens: the first item it can take (opened from the keyboard), the menu (from the pointer),
   * or nowhere (the gallery).
   */
  focus?: "item" | "menu" | "none";
  onClose: (how: MenuClose) => void;
  /** Elements a press on which is not a press away, such as the button that opened it, which toggles it. */
  inside?: () => (Element | null | undefined)[];
  /** For the gallery: the submenu, by its index in `entries`, shown open. */
  initialSubmenu?: number;
  /** A submenu: it leaves a column before its labels for the chosen option's check, and Left closes it. */
  nested?: boolean;
}

// A DOM without popovers (the tests') hides an element marked as one, so it is marked only where they exist.
const POPOVERS = typeof HTMLElement !== "undefined" && "showPopover" in HTMLElement.prototype;

const takes = (entry: MenuEntry) => entry.kind !== "rule" && entry.unavailable === undefined;

/**
 * A floating menu (docs/design/RAIL.md, Menus): a popover in the top layer, above the rail and its scrolling, that
 * fades in and out. The arrow keys move through the items it can take, Right and Left open and close a submenu,
 * Escape closes the innermost menu, and choosing an item closes every menu.
 */
export function Menu({ label, entries, open, place, focus = "item", onClose, inside, initialSubmenu, nested }: MenuProps) {
  const list = useRef<HTMLUListElement>(null);
  // Drawn while open, and while it fades out after; it goes at once with no animation, as with reduced motion.
  const [shown, setShown] = useState(open);
  if (open && !shown) setShown(true);
  useLayoutEffect(() => {
    const el = list.current;
    if (!el) return;
    // As a popover it sits in the top layer, above the rail's scrolling. A submenu rides in its menu's layer, where
    // the item it opens beside can place it. A DOM without popovers (the tests') shows it where it is.
    if (!nested && !isOpenPopover(el)) el.showPopover?.();
    if (open) return;
    let live = true;
    const done = () => live && setShown(false);
    Promise.all((el.getAnimations?.() ?? []).map((a) => a.finished)).then(done, done);
    return () => {
      live = false;
    };
  }, [open, shown]);

  // Focus moves in as it opens, as the opener asked.
  useEffect(() => {
    if (!open) return;
    const el = list.current;
    // With no item it can take, the menu itself, so its keys still reach it.
    if (focus === "item") (firstItem(el) ?? el)?.focus({ preventScroll: true });
    else if (focus === "menu") el?.focus({ preventScroll: true });
  }, [open, focus]);

  // A press anywhere but on the menu, its submenus or what opened it closes it, leaving focus where the press put it.
  useEffect(() => {
    if (!open || nested) return;
    const away = (event: PointerEvent) => {
      const target = event.target as Node;
      if (list.current?.contains(target) || inside?.().some((el) => el?.contains(target))) return;
      onClose("away");
    };
    document.addEventListener("pointerdown", away, true);
    return () => document.removeEventListener("pointerdown", away, true);
  }, [open, nested, onClose, inside]);

  const [submenu, setSubmenu] = useState<number | undefined>(initialSubmenu);
  // A submenu opened from the keyboard takes focus on its first item; one the pointer opens does not.
  const [subFocus, setSubFocus] = useState<"item" | "none">("none");
  const anchors = useId().replace(/[^a-zA-Z0-9]/g, "");
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);

  const move = (event: KeyboardEvent<HTMLUListElement>) => {
    const items = [...(list.current?.querySelectorAll<HTMLElement>(":scope > li > [data-takes]") ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    const go = (i: number) => items[(i + items.length) % items.length]?.focus({ preventScroll: true });
    switch (event.key) {
      case "ArrowDown":
        go(at + 1);
        break;
      case "ArrowUp":
        go(at < 0 ? -1 : at - 1);
        break;
      case "Home":
        go(0);
        break;
      case "End":
        go(-1);
        break;
      case "Escape":
        onClose("escape");
        break;
      case "ArrowLeft":
        // Left closes a submenu, back to its item; a top menu has nothing to its left.
        if (!nested) return;
        onClose("escape");
        break;
      default:
        return;
    }
    event.preventDefault();
    // A menu's keys are its own: Escape here does not also close the menu above it, or leave the screen.
    event.stopPropagation();
  };

  if (!shown) return null;
  const position = "anchor" in place ? ({ positionAnchor: place.anchor } as CSSProperties) : { left: place.x, top: place.y };
  return (
    <ul
      ref={list}
      className="menu"
      role="menu"
      aria-label={label}
      popover={nested || !POPOVERS ? undefined : "manual"}
      tabIndex={-1}
      data-place={"anchor" in place ? (nested ? "beside" : "below") : "point"}
      data-checks={nested || undefined}
      data-closing={!open || undefined}
      // While it fades out it takes no input.
      inert={!open}
      style={position}
      onKeyDown={move}
      onBlur={(event) => {
        // Focus left the menu, its submenus and what opened it for somewhere else in the window. Focus that goes
        // nowhere (a press on something that takes none) is the pointer handler's.
        const to = event.relatedTarget as Node | null;
        if (nested || to === null || list.current?.contains(to) || inside?.().some((el) => el?.contains(to))) return;
        onClose("away");
      }}
    >
      {entries.map((entry, i) => {
        if (entry.kind === "rule") return <li key={i} role="separator" className="menu-rule" />;
        const off = entry.unavailable !== undefined;
        const sub = entry.kind === "submenu";
        const subOpen = sub && submenu === i;
        const anchor = `--menu-${anchors}-${i}`;
        return (
          <li key={i} role="none">
            <button
              ref={(el) => {
                itemRefs.current[i] = el;
              }}
              type="button"
              role={nested && entry.kind === "item" ? "menuitemradio" : "menuitem"}
              className="menu-item"
              data-takes={takes(entry) || undefined}
              data-danger={(entry.kind === "item" && entry.danger) || undefined}
              data-open={subOpen || undefined}
              aria-disabled={off || undefined}
              aria-haspopup={sub ? "menu" : undefined}
              aria-expanded={sub ? subOpen : undefined}
              aria-checked={nested && entry.kind === "item" ? !!entry.checked : undefined}
              style={sub ? ({ anchorName: anchor } as CSSProperties) : undefined}
              onPointerEnter={() => {
                if (!sub || off) return;
                setSubFocus("none");
                setSubmenu(i);
              }}
              onKeyDown={(event) => {
                if (!sub || off || event.key !== "ArrowRight") return;
                event.preventDefault();
                event.stopPropagation();
                setSubFocus("item");
                setSubmenu(i);
              }}
              onClick={(event) => {
                if (off) return;
                if (sub) {
                  // A click the keyboard made (Enter or Space) has no pointer presses to count.
                  setSubFocus(event.detail === 0 ? "item" : "none");
                  setSubmenu(subOpen ? undefined : i);
                  return;
                }
                onClose("choose");
                entry.onChoose?.();
              }}
            >
              <span className="menu-row">
                {nested && <span className="menu-check">{entry.kind === "item" && entry.checked && <Icon icon={Check} size="icon-sm" />}</span>}
                <span className="menu-label">{entry.label}</span>
                {entry.kind === "item" && entry.keyHint && <span className="menu-key">{entry.keyHint}</span>}
                {sub && entry.current && <span className="menu-current">{entry.current}</span>}
                {sub && (
                  <span className="menu-chevron">
                    <Icon icon={ChevronRight} size="icon-sm" />
                  </span>
                )}
              </span>
              {/* An unavailable item says why beneath its label. */}
              {off && <span className="menu-reason">{entry.unavailable}</span>}
            </button>
            {sub && (
              <Menu
                label={entry.label}
                entries={entry.entries}
                open={subOpen && open}
                place={{ anchor }}
                focus={subFocus}
                nested
                onClose={(how) => {
                  setSubmenu(undefined);
                  // Escape or Left closes only the submenu, and focus goes back to its item; choosing closes them all.
                  if (how === "choose") onClose("choose");
                  else if (how === "escape") itemRefs.current[i]?.focus({ preventScroll: true });
                }}
              />
            )}
          </li>
        );
      })}
    </ul>
  );
}

/** The first item a menu can take focus on. */
function firstItem(menu: HTMLElement | null) {
  return menu?.querySelector<HTMLElement>(":scope > li > [data-takes]") ?? null;
}

/** Whether the popover is already in the top layer; a DOM without popovers has none to open. */
function isOpenPopover(el: HTMLElement) {
  try {
    return el.matches(":popover-open");
  } catch {
    return true;
  }
}
