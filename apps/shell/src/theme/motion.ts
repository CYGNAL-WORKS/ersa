// Motion read from the design tokens on the page, so the tokens stay the one source (docs/design/DESIGN.md, Motion).

/** Whether the user asked the system for reduced motion: then nothing slides or eases. */
export function reducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** A CSS time, such as a motion token's value, in milliseconds; 0 when it can't be read. */
export function milliseconds(time: string): number {
  const value = parseFloat(time);
  if (Number.isNaN(value)) return 0;
  return time.trim().endsWith("ms") ? value : value * 1000;
}

/** Where an element's layout puts it on the page, leaving out any slide still running, which transforms it. */
function topOf(el: HTMLElement): number {
  let top = 0;
  for (let at: Element | null = el; at instanceof HTMLElement; at = at.offsetParent) top += at.offsetTop;
  return top;
}

/** Where each element marked `data-slide` under `root` sits now, by its mark: taken before the rail changes. */
export function placesOf(root: HTMLElement): Map<string, number> {
  const places = new Map<string, number>();
  for (const el of root.querySelectorAll<HTMLElement>("[data-slide]")) places.set(el.dataset.slide ?? "", topOf(el));
  return places;
}

/** A move's duration and easing, named by their motion tokens. */
export interface Move {
  duration: `--motion-${string}`;
  easing: `--motion-${string}`;
}

/** How a tile moves when the user reorders the rail (DESIGN.md, Motion: "each tile slides to its new position"). */
export const SETTLE: Move = { duration: "--motion-settle", easing: "--motion-ease-in-out" };

/** How a new session's tile slides in, and the others make room (DESIGN.md, Motion: "A new session's tile"). */
export const ARRIVE: Move = { duration: "--motion-slow", easing: "--motion-ease-out" };

/**
 * Slides each element marked `data-slide` from where `before` had it to where it is now, over `move`. One that has
 * no place in `before` appears in place, unless its mark is in `arriving`: then it slides in from above by its own
 * height, fading in, over the same `move`. While they slide, `root` carries `data-sliding`, so its stylesheet can let
 * a tile show outside the frame it moved into.
 */
export function slideFrom(root: HTMLElement, before: ReadonlyMap<string, number>, move: Move = SETTLE, arriving?: ReadonlySet<string>): void {
  if (reducedMotion()) return;
  const style = getComputedStyle(root);
  const duration = milliseconds(style.getPropertyValue(move.duration));
  const easing = style.getPropertyValue(move.easing).trim();
  if (!duration || !easing) return;
  const slides: Animation[] = [];
  for (const el of root.querySelectorAll<HTMLElement>("[data-slide]")) {
    if (typeof el.animate !== "function") continue;
    const mark = el.dataset.slide ?? "";
    const from = before.get(mark);
    if (from === undefined) {
      if (arriving?.has(mark)) {
        const keyframes = [{ opacity: 0, transform: `translateY(${-el.offsetHeight}px)` }, { opacity: 1, transform: "none" }];
        slides.push(el.animate(keyframes, { duration, easing }));
      }
      continue;
    }
    const by = from - topOf(el);
    if (by !== 0) slides.push(el.animate([{ transform: `translateY(${by}px)` }, { transform: "none" }], { duration, easing }));
  }
  if (slides.length === 0) return;
  // A later slide takes over the mark; this one clears it only if it is still its own.
  const mark = String(performance.now());
  root.dataset.sliding = mark;
  Promise.allSettled(slides.map((slide) => slide.finished)).then(() => {
    if (root.dataset.sliding === mark) delete root.dataset.sliding;
  });
}

/**
 * Where each element marked `data-slide` under `root` sat before the elements in `arriving` were drawn: they are
 * taken out of the layout while the rest are measured, then put back. Read in a layout effect, before the page is
 * painted, so nothing flickers.
 */
export function placesWithout(root: HTMLElement, arriving: readonly HTMLElement[]): Map<string, number> {
  const shown = arriving.map((el) => el.style.display);
  for (const el of arriving) el.style.display = "none";
  const places = placesOf(root);
  arriving.forEach((el, i) => {
    el.style.display = shown[i] ?? "";
    // Taken out, it had no place: it has none to slide from.
    places.delete(el.dataset.slide ?? "");
  });
  return places;
}
