import { type RefObject, useLayoutEffect, useState } from "react";

/** Which side of what opened it a floating layer sits on, once the window has placed it. */
export type OpenSide = "above" | "below";

/**
 * The side a layer anchored to `anchor` opened on. A chip's list and the attach menu each have a usual side and flip
 * to the other where the window has no room (`position-try-fallbacks: flip-block`; a sideways flip, `flip-inline`,
 * moves it along its anchor and changes no side), and CSS cannot tell its entrance which one the window chose; so it
 * is measured once as the layer opens, after layout and before the first paint, and the entrance slides away from
 * what opened it on either side (docs/design/COMPOSER.md, A chip's list).
 *
 * Undefined until measured, and while closed.
 */
export function useOpenSide(layer: RefObject<HTMLElement | null>, anchor: RefObject<HTMLElement | null>, open: boolean) {
  const [side, setSide] = useState<OpenSide>();
  useLayoutEffect(() => {
    if (!open) {
      setSide(undefined);
      return;
    }
    const el = layer.current;
    const by = anchor.current;
    if (!el || !by) return;
    const a = el.getBoundingClientRect();
    const b = by.getBoundingClientRect();
    // The layer's middle above its anchor's: it opened above.
    setSide(a.top + a.bottom < b.top + b.bottom ? "above" : "below");
  }, [open, layer, anchor]);
  return side;
}
