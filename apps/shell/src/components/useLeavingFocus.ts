import { type FocusEvent, type RefObject, useLayoutEffect, useRef } from "react";

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
    const at = before.indexOf(gone);
    const after = before.slice(at + 1).find((id) => ids.includes(id));
    const earlier = before.slice(0, Math.max(at, 0)).reverse().find((id) => ids.includes(id));
    const to = after ?? earlier;
    const target = to !== undefined ? items[ids.indexOf(to)]?.querySelector<HTMLElement>(".session") : fallback(section);
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
