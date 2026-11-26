// @vitest-environment jsdom
// HeightEase toward a height its caller gives (docs/design/COMPOSER.md, The sign-in note: the room above the box
// eases to each new height over motion-settle, whatever changed it): a running ease toward the same height goes on,
// one toward another starts from where it has got to, growth is marked for clipping and shrinking is not, and with
// reduced motion the new height is taken at once.

import { afterEach, describe, expect, it, vi } from "vitest";
import { HeightEase } from "./motion";

/** An element whose height reads `height.now`, recording each ease it is given. */
function room() {
  const el = document.createElement("div");
  el.style.setProperty("--motion-settle", "260ms");
  el.style.setProperty("--motion-ease-out", "ease-out");
  document.body.append(el);
  const height = { now: 0 };
  el.getBoundingClientRect = () => ({ height: height.now }) as DOMRect;
  const runs: { frames: Keyframe[]; canceled: boolean; finish: () => void }[] = [];
  el.animate = ((frames: Keyframe[]) => {
    const run = { frames, canceled: false, finish: () => {} };
    const animation = { cancel: () => (run.canceled = true), onfinish: null as null | (() => void) };
    run.finish = () => animation.onfinish?.();
    runs.push(run);
    return animation;
  }) as unknown as HTMLElement["animate"];
  return { el, height, runs };
}

const motion = (reduce: boolean) => vi.stubGlobal("matchMedia", (query: string) => ({ matches: reduce && query.includes("reduce") }));

describe("an ease toward a height its caller gives", () => {
  afterEach(() => {
    document.body.replaceChildren();
    vi.unstubAllGlobals();
  });

  it("eases from the last height to the new one over motion-settle, marking growth for clipping", () => {
    motion(false);
    const { el, runs } = room();
    const ease = new HeightEase();
    ease.change(el, true, { to: 0, clipGrowth: true });
    ease.change(el, true, { to: 58, clipGrowth: true });
    expect(runs.map((r) => r.frames)).toEqual([[{ height: "0px" }, { height: "58px" }]]);
    expect(el.dataset.easing).toBe("");
    runs[0]!.finish();
    expect(el.dataset.easing).toBeUndefined();
  });

  it("goes on with a running ease toward the same height, and starts afresh from where it got to for another", () => {
    motion(false);
    const { el, height, runs } = room();
    const ease = new HeightEase();
    ease.change(el, true, { to: 0 });
    ease.change(el, true, { to: 58 });
    ease.change(el, true, { to: 58 });
    expect(runs).toHaveLength(1);
    height.now = 30;
    ease.change(el, true, { to: 83 });
    expect(runs[0]!.canceled).toBe(true);
    expect(runs[1]!.frames).toEqual([{ height: "30px" }, { height: "83px" }]);
  });

  it("does not mark a shrinking room, so what overflows it upward stays in view", () => {
    motion(false);
    const { el, runs } = room();
    const ease = new HeightEase();
    ease.change(el, true, { to: 58 });
    ease.change(el, true, { to: 0, clipGrowth: true });
    expect(runs.map((r) => r.frames)).toEqual([[{ height: "58px" }, { height: "0px" }]]);
    expect(el.dataset.easing).toBeUndefined();
  });

  it("takes the new height at once with reduced motion", () => {
    motion(true);
    const { el, runs } = room();
    const ease = new HeightEase();
    ease.change(el, true, { to: 0, clipGrowth: true });
    ease.change(el, true, { to: 58, clipGrowth: true });
    expect(runs).toHaveLength(0);
    expect(el.dataset.easing).toBeUndefined();
  });
});
