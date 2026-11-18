import { type FocusEvent, type RefObject, useLayoutEffect, useRef } from "react";

/** Whether a tile is folded away in a collapsed group, where it takes no focus. */
export function foldedAway(tile: Element): boolean {
  return tile.closest(".rail-group-tiles[inert]") !== null;
}

/**
 * Where focus goes when `gone` leaves (docs/design/RAIL.md, the rule after a deletion): the next of `before`, the order
 * drawn last, that is still `open` to focus, else the previous one; undefined when none is, for the caller's fallback.
 */
export function besideGone(before: readonly string[], gone: string, open: readonly string[]): string | undefined {
  const at = before.indexOf(gone);
  const after = before.slice(at + 1).find((id) => open.includes(id));
  return after ?? before.slice(0, Math.max(at, 0)).reverse().find((id) => open.includes(id));
}

/**
 * A list item (`[data-session-id]`) that leaves while keyboard focus is in it, as a session archived from its menu
 * does, hands focus to the next item, else the previous one, else `fallback`, unless the user has put focus somewhere
 * else meanwhile (docs/design/RAIL.md, the rule after a deletion). The items drawn last time give the order. Spread
 * the result on `root`.
 */
export function useLeavingFocus(root: RefObject<HTMLElement | null>, fallback: (root: HTMLElement) => HTMLElement | null | undefined) {
  const drawn = useRef<string[]>([]);
  const focused = useRef<string | undefined>(undefined);
  useLayoutEffect(() => {
    const section = root.current;
    if (!section) return;
    const items = [...section.querySelectorAll<HTMLElement>("[data-session-id]")];
    const ids = items.map((el) => el.dataset.sessionId ?? "");
    const before = drawn.current;
    drawn.current = ids;
    const gone = focused.current;
    if (gone === undefined || ids.includes(gone)) return;
    focused.current = undefined;
    const now = document.activeElement;
    if (now !== null && now !== document.body) return;
    // An item folded away can't take focus, so it is never the one beside this one.
    const open = items.filter((el) => !foldedAway(el));
    const openIds = open.map((el) => el.dataset.sessionId ?? "");
    const to = besideGone(before, gone, openIds);
    const target = to !== undefined ? open[openIds.indexOf(to)]?.querySelector<HTMLElement>(".session") : fallback(section);
    target?.focus();
  });
  return {
    onFocus: (event: FocusEvent) => {
      focused.current = (event.target as Element).closest<HTMLElement>("[data-session-id]")?.dataset.sessionId;
    },
    // Focus put outside is the user's: an item leaving later takes nothing back.
    onBlur: (event: FocusEvent) => {
      if (event.relatedTarget !== null && !root.current?.contains(event.relatedTarget)) focused.current = undefined;
    },
  };
}
