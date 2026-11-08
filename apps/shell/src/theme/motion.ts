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

/**
 * Slides each element marked `data-slide` from where `before` had it to where it is now, over `motion-settle` with
 * `motion-ease-in-out` (DESIGN.md, Motion: "each tile slides to its new position"). One that is new appears in place.
 * While they slide, `root` carries `data-sliding`, so its stylesheet can let a tile show outside the frame it moved
 * into.
 */
export function slideFrom(root: HTMLElement, before: ReadonlyMap<string, number>): void {
  if (reducedMotion()) return;
  const style = getComputedStyle(root);
  const duration = milliseconds(style.getPropertyValue("--motion-settle"));
  const easing = style.getPropertyValue("--motion-ease-in-out").trim();
  if (!duration || !easing) return;
  const slides: Animation[] = [];
  for (const el of root.querySelectorAll<HTMLElement>("[data-slide]")) {
    const from = before.get(el.dataset.slide ?? "");
    if (from === undefined || typeof el.animate !== "function") continue;
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
