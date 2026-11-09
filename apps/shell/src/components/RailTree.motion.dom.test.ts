// @vitest-environment jsdom
// The rail's motion (docs/design/DESIGN.md, Motion; docs/design/RAIL.md): a working session's glyph breathes, a session
// that comes to need the user breathes once where it is, and a new session's tile slides in while the others make
// room. A design rule, so no requirement IDs. jsdom runs no CSS animation: these check what the stylesheet animates
// (the marks) and what the rail asks the page to animate (the slides); the gallery checks the stylesheet itself.

import { act, cleanup, render } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Project, Session } from "../protocol/core-api.gen";
import { RailTree } from "./RailTree";

const session = (id: string, title: string, created_at: string, tile: Session["tile"] = { state: "idle", pending: [] }, workspace_id = "w"): Session => ({
  id,
  title,
  workspace_id,
  host_id: "local",
  backend: "claude-code",
  model: "opus",
  effort: "medium",
  permission_mode: "default",
  state: tile.state,
  created_at,
  archived: false,
  pinned: false,
  tile,
});

const working: Session["tile"] = { state: "running", pending: [] };
const asking: Session["tile"] = { state: "waiting_user", pending: [{ kind: "permission", request_id: "r" }] };

const project: Project = { id: "p", host_id: "local", path: "D:/p", name: "p", vcs: "git", trusted: true };

const rail = (sessions: Session[], activeId?: string, more: Partial<Parameters<typeof RailTree>[0]> = {}) =>
  createElement(RailTree, {
    projects: { state: "ready", projects: [project] },
    workspaces: { w: { project_id: "p" } },
    sessions: { state: "ready", sessions },
    activeId,
    onSelect: () => {},
    ...more,
  });

afterEach(cleanup);

describe("a working session's glyph", () => {
  it("breathes while the session works and stops when it stops", () => {
    const { container, rerender } = render(rail([session("a", "Fix the build", "2026-11-06T09:00:00Z", working)]));
    expect(container.querySelector(".session-glyph")?.getAttribute("data-breath")).toBe("loop");
    rerender(rail([session("a", "Fix the build", "2026-11-06T09:00:00Z", { state: "disconnected", pending: [] })]));
    expect(container.querySelector(".session-glyph")?.hasAttribute("data-breath")).toBe(false);
  });
});

describe("a session that comes to need the user", () => {
  const chevron = (container: HTMLElement) => container.querySelector<HTMLElement>(".session-chevron-button > *, .session-chevron");

  it("breathes its chevron once, where the tile is", () => {
    const { container, rerender } = render(rail([session("b", "Write", "2026-11-06T08:00:00Z"), session("a", "Fix", "2026-11-06T09:00:00Z", working)]));
    const order = () => [...container.querySelectorAll<HTMLElement>("[data-session-id]")].map((el) => el.dataset.sessionId);
    const before = order();
    rerender(rail([session("b", "Write", "2026-11-06T08:00:00Z"), session("a", "Fix", "2026-11-06T09:00:00Z", asking)]));
    expect(container.querySelector(".session-chevron-button")?.getAttribute("data-breath")).toBe("once");
    expect(order()).toEqual(before);
    // One breath: once it ends, the chevron holds still.
    // jsdom has no AnimationEvent, so React listens for the prefixed end event, and the name is set on it here.
    for (const type of ["animationend", "webkitAnimationEnd"]) {
      const end = new Event(type, { bubbles: true });
      Object.defineProperty(end, "animationName", { value: "session-breath" });
      act(() => {
        chevron(container)?.dispatchEvent(end);
      });
    }
    expect(container.querySelector("[data-breath]")).toBeNull();
  });

  it("breathes the active tile's chevron mark too", () => {
    const { container, rerender } = render(rail([session("a", "Fix", "2026-11-06T09:00:00Z", working)], "a"));
    rerender(rail([session("a", "Fix", "2026-11-06T09:00:00Z", asking)], "a"));
    expect(container.querySelector(".session-chevron")?.getAttribute("data-breath")).toBe("once");
  });

  it("ends the breath when the tile is made active partway through, so the chevron drawn there doesn't start it over", () => {
    const { container, rerender } = render(rail([session("a", "Fix", "2026-11-06T09:00:00Z", working)]));
    rerender(rail([session("a", "Fix", "2026-11-06T09:00:00Z", asking)]));
    expect(container.querySelector(".session-chevron-button")?.getAttribute("data-breath")).toBe("once");
    rerender(rail([session("a", "Fix", "2026-11-06T09:00:00Z", asking)], "a"));
    expect(container.querySelector(".session-chevron")).not.toBeNull();
    expect(container.querySelector("[data-breath]")).toBeNull();
  });

  it("leaves nothing waiting to breathe with reduced motion on", () => {
    window.matchMedia = ((query: string) => ({ matches: query.includes("reduce") })) as never;
    try {
      const { container, rerender } = render(rail([session("a", "Fix", "2026-11-06T09:00:00Z", working)]));
      rerender(rail([session("a", "Fix", "2026-11-06T09:00:00Z", asking)]));
      expect(container.querySelector(".session-chevron-button")).not.toBeNull();
      expect(container.querySelector("[data-breath]")).toBeNull();
    } finally {
      delete (window as { matchMedia?: unknown }).matchMedia;
    }
  });

  it("shows a chevron at once when the tile is first drawn needing the user", () => {
    const { container } = render(rail([session("a", "Fix", "2026-11-06T09:00:00Z", asking)]));
    expect(container.querySelector(".session-chevron-button")).not.toBeNull();
    expect(container.querySelector("[data-breath]")).toBeNull();
  });
});

describe("a new session's tile", () => {
  const slid: { slide: string | undefined; keyframes: Keyframe[]; options: KeyframeAnimationOptions }[] = [];
  let reduced = false;
  beforeEach(() => {
    slid.length = 0;
    reduced = false;
    document.documentElement.style.setProperty("--motion-slow", "400ms");
    document.documentElement.style.setProperty("--motion-ease-out", "ease-out");
    document.documentElement.style.setProperty("--motion-settle", "260ms");
    document.documentElement.style.setProperty("--motion-ease-in-out", "ease-in-out");
    window.matchMedia = ((query: string) => ({ matches: reduced && query.includes("reduce") })) as never;
    // jsdom lays nothing out: each tile shown sits at its order among the tiles shown, 10 apart and 10 high.
    const shown = () => [...document.querySelectorAll<HTMLElement>("[data-slide]")].filter((el) => el.style.display !== "none");
    vi.spyOn(HTMLElement.prototype, "offsetTop", "get").mockImplementation(function (this: HTMLElement) {
      return shown().indexOf(this) * 10;
    });
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(10);
    HTMLElement.prototype.animate = function (this: HTMLElement, keyframes: Keyframe[], options: KeyframeAnimationOptions) {
      slid.push({ slide: this.dataset.slide, keyframes, options });
      return { finished: Promise.resolve() } as unknown as Animation;
    } as never;
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    delete (HTMLElement.prototype as { animate?: unknown }).animate;
    delete (window as { matchMedia?: unknown }).matchMedia;
    document.documentElement.removeAttribute("style");
  });

  const older = session("a", "Fix", "2026-11-06T08:00:00Z");
  const newer = session("b", "Write", "2026-11-06T09:00:00Z");

  it("draws the first listing in place", () => {
    render(rail([older, newer]));
    expect(slid).toEqual([]);
  });

  it("slides in at its place over motion-slow with motion-ease-out, while the others make room over the same", () => {
    const { rerender } = render(rail([older]));
    // Newest first: the new tile takes the top, and the older one moves down a place.
    rerender(rail([older, newer]));
    const options = { duration: 400, easing: "ease-out" };
    expect(slid).toEqual([
      { slide: "session:b", keyframes: [{ opacity: 0, transform: "translateY(-10px)" }, { opacity: 1, transform: "none" }], options },
      { slide: "session:a", keyframes: [{ transform: "translateY(-10px)" }, { transform: "none" }], options },
    ]);
  });

  it("slides in once it is drawn when it had to wait to be placed", () => {
    const placed = session("b", "Write", "2026-11-06T09:00:00Z", undefined, "w2");
    const known = { workspaces: { w: { project_id: "p" }, w2: { project_id: "p" } } };
    const { rerender } = render(rail([older]));
    // Its workspace is still being read: it isn't drawn, so nothing slides yet.
    rerender(rail([older, placed], undefined, { placing: true }));
    expect(slid).toEqual([]);
    rerender(rail([older, placed], undefined, known));
    expect(slid.map((s) => s.slide)).toEqual(["session:b", "session:a"]);
    expect(slid[0]?.keyframes[0]).toEqual({ opacity: 0, transform: "translateY(-10px)" });
    // Spent: another drawing brings nothing new.
    slid.length = 0;
    rerender(rail([older, placed], "a", known));
    expect(slid).toEqual([]);
  });

  it("waits for the read of its workspace, while a tile already drawn at the rail's end stays where it is", () => {
    const lost = session("c", "Lost", "2026-11-06T07:00:00Z", undefined, "w-lost");
    const fresh = session("b", "Write", "2026-11-06T09:00:00Z", undefined, "w2");
    const { container, rerender } = render(rail([older, lost]));
    const drawn = container.querySelector('[data-session-id="c"]');
    expect(drawn).not.toBeNull();
    rerender(rail([older, lost, fresh], undefined, { reading: true }));
    expect(container.querySelector('[data-session-id="b"]')).toBeNull();
    // Drawn again while the read is out, as any change to the rail does: it still waits.
    rerender(rail([older, lost, fresh], undefined, { reading: true }));
    expect(container.querySelector('[data-session-id="b"]')).toBeNull();
    expect(container.querySelector('[data-session-id="c"]')).toBe(drawn);
    expect(slid).toEqual([]);
    rerender(rail([older, lost, fresh]));
    expect(slid[0]?.slide).toBe("session:b");
  });

  it("stays drawn through a read that comes after it has slid in", () => {
    const lost = session("b", "Write", "2026-11-06T09:00:00Z", undefined, "w2");
    const { container, rerender } = render(rail([older]));
    rerender(rail([older, lost]));
    const drawn = container.querySelector('[data-session-id="b"]');
    expect(drawn).not.toBeNull();
    rerender(rail([older, lost], undefined, { reading: true }));
    expect(container.querySelector('[data-session-id="b"]')).toBe(drawn);
  });

  it("slides in at the rail's end when no project will hold it", () => {
    const lost = session("b", "Write", "2026-11-06T09:00:00Z", undefined, "w2");
    const { rerender } = render(rail([older]));
    rerender(rail([older, lost]));
    expect(slid.map((s) => s.slide)).toEqual(["session:b"]);
  });

  it("makes room by its offset on a tile with its shelf open, so the shelf stays on its anchor", () => {
    vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
    const shelf = { id: "a", tone: "caution" as const, render: () => createElement("div", { className: "shelf" }) };
    const { container, rerender } = render(rail([older], undefined, { shelf }));
    expect(container.querySelector(".shelf")).not.toBeNull();
    rerender(rail([older, newer], undefined, { shelf }));
    expect(slid.map((s) => [s.slide, s.keyframes])).toEqual([
      ["session:b", [{ opacity: 0, transform: "translateY(-10px)" }, { opacity: 1, transform: "none" }]],
      ["session:a", [{ top: "-10px" }, { top: "0" }]],
    ]);
  });

  it("brings nothing new when a session listed before comes back, as an unarchived one does", () => {
    const { rerender } = render(rail([older, newer]));
    rerender(rail([older]));
    rerender(rail([older, newer]));
    expect(slid).toEqual([]);
  });

  it("brings nothing new when a session first listed archived is unarchived", () => {
    const { rerender } = render(rail([older]));
    rerender(rail([older, { ...newer, archived: true }]));
    rerender(rail([older, newer]));
    expect(slid).toEqual([]);
  });

  describe("when a tile changes place in the sort", () => {
    const settle = { duration: 260, easing: "ease-in-out" };
    const sorted = (sessions: Session[], sortBy: "created" | "last_activity" | "name") => rail(sessions, undefined, { sortBy });
    const ended = { ...older, last_activity_at: "2026-11-06T10:00:00Z" };
    // The tiles swap: the project's heading is first, then the two tiles, each 10 high.
    const swapped = [
      { slide: "session:a", keyframes: [{ transform: "translateY(10px)" }, { transform: "none" }], options: settle },
      { slide: "session:b", keyframes: [{ transform: "translateY(-10px)" }, { transform: "none" }], options: settle },
    ];

    // verifies: FLOW-ORGANIZE-SESSIONS-R10
    it("slides each tile over motion-settle when a turn ends while sorted by last activity", () => {
      const { rerender } = render(sorted([older, newer], "last_activity"));
      rerender(sorted([ended, newer], "last_activity"));
      expect(slid).toEqual(swapped);
    });

    // verifies: FLOW-ORGANIZE-SESSIONS-R10
    it("moves nothing as a session works, or when a turn ends under another sort", () => {
      const { container, rerender } = render(sorted([older, newer], "last_activity"));
      const order = () => [...container.querySelectorAll<HTMLElement>("[data-session-id]")].map((el) => el.dataset.sessionId);
      rerender(sorted([{ ...older, state: "running", last_event_seq: 9, tile: working }, newer], "last_activity"));
      expect(order()).toEqual(["b", "a"]);
      expect(slid).toEqual([]);
      const { rerender: again } = render(sorted([older, newer], "created"));
      again(sorted([ended, newer], "created"));
      expect(slid).toEqual([]);
    });

    it("slides each tile when the user chooses another sort", () => {
      const { rerender } = render(sorted([older, newer], "last_activity"));
      expect(slid).toEqual([]);
      rerender(sorted([older, newer], "name"));
      expect(slid).toEqual(swapped);
    });

    it("slides a tile a new title moves while sorted by name", () => {
      const { rerender } = render(sorted([older, newer], "name"));
      rerender(sorted([older, { ...newer, title: "Ask" }], "name"));
      expect(slid).toEqual([
        { slide: "session:b", keyframes: [{ transform: "translateY(10px)" }, { transform: "none" }], options: settle },
        { slide: "session:a", keyframes: [{ transform: "translateY(-10px)" }, { transform: "none" }], options: settle },
      ]);
    });

    it("draws tiles coming back after Core was lost in place", () => {
      const { rerender } = render(sorted([older, newer], "last_activity"));
      rerender(rail([], undefined, { sortBy: "last_activity", sessions: { state: "loading" } }));
      rerender(sorted([ended, newer], "last_activity"));
      expect(slid).toEqual([]);
    });

    it("keeps keyboard focus on a tile another session's move makes React put back in the page", () => {
      const first = session("x", "First", "2026-11-06T09:00:00Z");
      const second = session("y", "Second", "2026-11-06T08:00:00Z");
      const third = session("z", "Third", "2026-11-06T07:00:00Z");
      const { container, rerender } = render(sorted([first, second, third], "last_activity"));
      const order = () => [...container.querySelectorAll<HTMLElement>("[data-session-id]")].map((el) => el.dataset.sessionId);
      expect(order()).toEqual(["x", "y", "z"]);
      const tile = container.querySelector<HTMLElement>('[data-session-id="x"] .session');
      tile?.focus();
      const item = container.querySelector('[data-session-id="x"]');
      // z comes first, so React keeps z's element where it is and moves x's and y's after it.
      rerender(sorted([first, second, { ...third, last_activity_at: "2026-11-06T10:00:00Z" }], "last_activity"));
      expect(order()).toEqual(["z", "x", "y"]);
      expect(container.querySelector('[data-session-id="x"]')).toBe(item);
      expect(document.activeElement).toBe(tile);
    });

    it("takes the new places at once with reduced motion on", () => {
      reduced = true;
      const { rerender } = render(sorted([older, newer], "last_activity"));
      rerender(sorted([ended, newer], "last_activity"));
      expect(slid).toEqual([]);
    });
  });

  it("appears in place with reduced motion on", () => {
    reduced = true;
    const { container, rerender } = render(rail([older]));
    rerender(rail([older, newer]));
    expect(slid).toEqual([]);
    // Measured and put back: nothing is left hidden.
    expect([...container.querySelectorAll<HTMLElement>("[data-slide]")].every((el) => el.style.display === "")).toBe(true);
  });
});
