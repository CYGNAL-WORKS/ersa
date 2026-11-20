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

const idOf = (item: HTMLElement) => item.dataset.sessionId ?? "";

/**
 * The list item (`[data-session-id]`) in `list` that takes focus when `gone` leaves it: the next, else the previous,
 * in `before` (the order drawn before it left; the order drawn now when not given), past any folded away, which can't
 * take focus. Undefined when none is left. The one place delete, archive and any other leaving find it.
 */
export function besideIn(list: Element, gone: string, before?: readonly string[]): HTMLElement | undefined {
  const items = [...list.querySelectorAll<HTMLElement>("[data-session-id]")];
  const open = items.filter((item) => !foldedAway(item) && idOf(item) !== gone);
  const openIds = open.map(idOf);
  const to = besideGone(before ?? items.map(idOf), gone, openIds);
  return to === undefined ? undefined : open[openIds.indexOf(to)];
}

/** What a restore focuses once the list has redrawn, or "wait" while what it waits for has not happened yet. */
export type RestoreTarget = HTMLElement | null | undefined | "wait";

/**
 * Gives keyboard focus back as a list (`root`) redraws, the one place this happens (docs/design/RAIL.md: a tile that
 * leaves the rail, and Menus), and only while the user has left focus with the list: focus put elsewhere, or a press
 * on something that takes none, is the user's, and nothing is taken back.
 *
 * - An item (`[data-session-id]`) that leaves while focus is in it, as a session archived from its menu does, hands
 *   focus to the next item, else the previous one, else `fallback`.
 * - `restore(find)` asks for focus to go, once the list redraws, to what `find` names: a tile drawn afresh in its new
 *   group, a new heading. `find` is asked after each redraw until it stops waiting.
 *
 * Spread `handlers` on `root`.
 */
export function useFocusRestore(root: RefObject<HTMLElement | null>, fallback: (root: HTMLElement) => HTMLElement | null | undefined) {
  const drawn = useRef<string[]>([]);
  // The item focus is in, and the restore asked for, until it is done or the user takes focus elsewhere.
  const focused = useRef<string | undefined>(undefined);
  const pending = useRef<(root: HTMLElement) => RestoreTarget>(undefined);
  useLayoutEffect(() => {
    const list = root.current;
    if (!list) return;
    const ids = [...list.querySelectorAll<HTMLElement>("[data-session-id]")].map(idOf);
    const before = drawn.current;
    drawn.current = ids;
    const gone = focused.current;
    if (gone !== undefined && !ids.includes(gone)) {
      focused.current = undefined;
      // The item with focus has gone: that wins over anything asked for meanwhile.
      pending.current = (el) => besideIn(el, gone, before)?.querySelector<HTMLElement>(".session") ?? fallback(el);
    }
    const find = pending.current;
    if (!find) return;
    const target = find(list);
    if (target === "wait") return;
    pending.current = undefined;
    // Focus the user has put somewhere since stays there.
    const now = document.activeElement;
    if (now !== null && now !== document.body) return;
    target?.focus({ preventScroll: true });
  });
  const release = () => {
    focused.current = undefined;
    pending.current = undefined;
  };
  return {
    restore: (find: (root: HTMLElement) => RestoreTarget) => {
      pending.current = find;
    },
    handlers: {
      onFocus: (event: FocusEvent) => {
        focused.current = (event.target as Element).closest<HTMLElement>("[data-session-id]")?.dataset.sessionId;
      },
      onBlur: (event: FocusEvent) => {
        const to = event.relatedTarget;
        // Focus put outside is the user's.
        if (to !== null) {
          if (!root.current?.contains(to)) release();
          return;
        }
        // Focus going to nothing is the user's too when what had it is still drawn: a press on something that takes
        // no focus. One the list removed as it redrew is no longer drawn, and its focus is given back. A window
        // losing focus keeps the element as the document's focus, so that releases nothing.
        const from = event.target as Element;
        queueMicrotask(() => {
          const now = document.activeElement;
          if (from.isConnected && now !== from && !(now && root.current?.contains(now))) release();
        });
      },
    },
  };
}
