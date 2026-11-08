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

/** Where a marked element sat, and the element, which fades out from there if the change takes it away. */
export interface Place {
  top: number;
  el: HTMLElement;
}

/** Where each element marked `data-slide` under `root` sits now, by its mark: taken before the rail changes. */
export function placesOf(root: HTMLElement): Map<string, Place> {
  const places = new Map<string, Place>();
  for (const el of root.querySelectorAll<HTMLElement>("[data-slide]")) places.set(el.dataset.slide ?? "", { top: topOf(el), el });
  return places;
}

/**
 * Fades out an element React has just removed, where it sat: put back under `root`, out of the layout (the rail's
 * stylesheet places `data-leaving`), out of reach, and with its marks gone so nothing takes it for what is on the page.
 */
function fadeOut(root: HTMLElement, el: HTMLElement, top: number, timing: KeyframeAnimationOptions): void {
  if (typeof el.animate !== "function") return;
  for (const marked of [el, ...el.querySelectorAll<HTMLElement>("[data-slide], [data-session-id]")]) {
    delete marked.dataset.slide;
    delete marked.dataset.sessionId;
  }
  el.inert = true;
  el.setAttribute("aria-hidden", "true");
  el.dataset.leaving = "";
  el.style.top = `${top - topOf(root)}px`;
  root.append(el);
  const remove = () => el.remove();
  el.animate([{ opacity: 1 }, { opacity: 0 }], timing).finished.then(remove, remove);
}

/**
 * Slides each element marked `data-slide` from where `before` had it to where it is now, over `motion-settle` with
 * `motion-ease-in-out` (DESIGN.md, Motion: "each tile slides to its new position"). One that is new fades in where it
 * is, and one that has gone fades out where it was (DESIGN.md: nothing pops in). While they slide, `root` carries `data-sliding`, so its stylesheet can let a tile show outside the frame it moved
 * into.
 */
export function slideFrom(root: HTMLElement, before: ReadonlyMap<string, Place>): void {
  if (reducedMotion()) return;
  const style = getComputedStyle(root);
  const duration = milliseconds(style.getPropertyValue("--motion-settle"));
  const easing = style.getPropertyValue("--motion-ease-in-out").trim();
  if (!duration || !easing) return;
  const slides: Animation[] = [];
  const marks = new Set<string>();
  const timing = { duration, easing };
  for (const el of root.querySelectorAll<HTMLElement>("[data-slide]")) {
    const mark = el.dataset.slide ?? "";
    marks.add(mark);
    if (typeof el.animate !== "function") continue;
    const from = before.get(mark);
    if (from === undefined) {
      el.animate([{ opacity: 0 }, { opacity: 1 }], timing);
      continue;
    }
    const by = from.top - topOf(el);
    if (by !== 0) slides.push(el.animate([{ transform: `translateY(${by}px)` }, { transform: "none" }], timing));
  }
  for (const [mark, place] of before) if (!marks.has(mark) && !place.el.isConnected) fadeOut(root, place.el, place.top, timing);
  if (slides.length === 0) return;
  // A later slide takes over the mark; this one clears it only if it is still its own.
  const mark = String(performance.now());
  root.dataset.sliding = mark;
  Promise.allSettled(slides.map((slide) => slide.finished)).then(() => {
    if (root.dataset.sliding === mark) delete root.dataset.sliding;
  });
}
