// @vitest-environment jsdom
// What moves when the rail regroups (DESIGN.md: nothing pops in; FLOW-ORGANIZE-SESSIONS, step 4).

import { afterEach, describe, expect, it, vi } from "vitest";
import { placesOf, slideFrom } from "./motion";

/** Records each animation on `el`; its `finish` ends the animation as the browser would. */
function animated(el: HTMLElement) {
  const runs: { frames: Keyframe[]; finish: () => void }[] = [];
  el.animate = ((frames: Keyframe[]) => {
    let finish = () => {};
    const finished = new Promise<void>((resolve) => (finish = resolve));
    runs.push({ frames, finish: () => finish() });
    return { finished };
  }) as unknown as HTMLElement["animate"];
  return runs;
}

function marked(root: HTMLElement, mark: string) {
  const el = document.createElement("div");
  el.dataset.slide = mark;
  el.dataset.sessionId = mark;
  root.append(el);
  return { el, runs: animated(el) };
}

function rail() {
  const root = document.createElement("section");
  root.style.setProperty("--motion-settle", "300ms");
  root.style.setProperty("--motion-ease-in-out", "ease-in-out");
  document.body.append(root);
  return root;
}

describe("what the rail does as it regroups", () => {
  afterEach(() => {
    document.body.replaceChildren();
    vi.unstubAllGlobals();
  });

  it("fades in what is new", () => {
    const root = rail();
    const before = placesOf(root);
    const { runs } = marked(root, "heading:today");
    slideFrom(root, before);
    expect(runs.map((r) => r.frames)).toEqual([[{ opacity: 0 }, { opacity: 1 }]]);
  });

  it("fades out what has gone where it sat, out of reach and no longer marked, then takes it away", async () => {
    const root = rail();
    const { el, runs } = marked(root, "session:s1");
    const before = placesOf(root);
    el.remove();
    slideFrom(root, before);
    expect(el.parentElement).toBe(root);
    expect(el.inert).toBe(true);
    expect(el.dataset.leaving).toBe("");
    expect(el.dataset.slide).toBeUndefined();
    expect(el.dataset.sessionId).toBeUndefined();
    expect(runs.map((r) => r.frames)).toEqual([[{ opacity: 1 }, { opacity: 0 }]]);
    runs[0]!.finish();
    await Promise.resolve();
    await Promise.resolve();
    expect(el.isConnected).toBe(false);
  });

  it("does neither with reduced motion on", () => {
    vi.stubGlobal("matchMedia", () => ({ matches: true }));
    const root = rail();
    const { el, runs } = marked(root, "session:s1");
    const before = placesOf(root);
    el.remove();
    const added = marked(root, "heading:today");
    slideFrom(root, before);
    expect(el.isConnected).toBe(false);
    expect(runs).toEqual([]);
    expect(added.runs).toEqual([]);
  });
});
