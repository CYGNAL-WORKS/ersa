// @vitest-environment jsdom
// The workbench in the window (FLOW-READ-RAW-LOG, step 1; R01, R02, R16) against a fake Core: Developer pane in the
// arrange menu, the pane's Close, the open state and width kept in Core's settings, and the resize edge.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, CoreEvent, Method } from "./core/client";
import { mergePatch } from "./session/arrangement";
import type { CoreHelloResult } from "./protocol/core-api.gen";

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

type Answers = Partial<Record<Method, (params: never) => unknown>>;

/**
 * A Core with no sessions that keeps settings as Core does, each patch merged in; `refuse` makes it refuse every
 * `settings.set`. It attaches again after a drop, so the window reads the settings again.
 */
function fakeCore(kept: unknown = {}, { refuse = false } = {}) {
  let settings = kept;
  let onEvent: (event: CoreEvent) => void = () => {};
  const calls: { method: Method; params: unknown }[] = [];
  const answers: Answers = {
    "core.status": () => ({ sessions_running: 0, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => [],
    "session.list": () => [],
    "backend.list": () => [],
    "section.list": () => [],
    "settings.get": () => settings,
    "settings.set": (params: { patch: unknown }) => {
      if (refuse) throw { kind: "core", code: "unavailable", message: "a replay keeps no settings" };
      settings = mergePatch(settings, params.patch);
      return {};
    },
  };
  const client: CoreClient = {
    attach: (listener) => {
      onEvent = listener;
      return Promise.resolve<Attachment>({ how: "attached", hello });
    },
    call: (method, params) => {
      calls.push({ method, params });
      const answer = answers[method];
      if (!answer) return Promise.reject({ kind: "unavailable", message: `no answer for ${method}` }) as never;
      return new Promise((resolve) => resolve(answer(params as never))) as never;
    },
  };
  const writes = () => calls.filter((c) => c.method === "settings.set").map((c) => (c.params as { patch: unknown }).patch);
  const reads = () => calls.filter((c) => c.method === "settings.get").length;
  return { client, writes, reads, settings: () => settings, drop: () => act(() => onEvent({ kind: "lost", reason: "gone" })) };
}

const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));

async function start(core: ReturnType<typeof fakeCore>) {
  const shown = render(createElement(App, { client: core.client }));
  await settle();
  await settle();
  return shown;
}

const rail = () => screen.getByRole("navigation", { name: "Rail" });
const arrangeButton = () => within(rail()).getByRole("button", { name: "Arrange sessions" });
const workbench = () => screen.queryByRole("complementary", { name: "Developer" });
const edge = () => screen.getByRole("separator", { name: "Resize workbench" });
/** Opens the arrange menu and returns its Developer pane item. */
function developerPane() {
  fireEvent.click(arrangeButton());
  return screen.getByRole("menuitemcheckbox", { name: "Developer pane" });
}

// The design's tokens as the page has them, and a window whose region beside the rail is `shared` pixels wide.
let shared = 1200;
beforeEach(() => {
  const root = document.documentElement.style;
  root.setProperty("--size-workbench", "480px");
  root.setProperty("--size-workbench-min", "320px");
  root.setProperty("--size-workbench-max", "50%");
  root.setProperty("--space-lg", "16px");
  shared = 1200;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return DOMRect.fromRect({ width: this.classList.contains("work") ? shared : 0, height: 0 });
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("opening and closing the workbench", () => {
  // verifies: FLOW-READ-RAW-LOG-R01
  it("opens from Developer pane in the arrange menu, marks the item while open, and closes when it is chosen again", async () => {
    await start(fakeCore());
    expect(workbench()).toBeNull();
    const item = developerPane();
    expect(item.getAttribute("aria-checked")).toBe("false");
    // Developer pane is the menu's last item, apart from the rest.
    const items = screen.getAllByRole("menu")[0]!.querySelectorAll("li");
    expect(items[items.length - 1]?.textContent).toBe("Developer pane");
    expect(items[items.length - 2]?.getAttribute("role")).toBe("separator");
    fireEvent.click(item);
    expect(workbench()).not.toBeNull();
    expect(within(workbench()!).getByRole("heading", { name: "Developer" })).toBeTruthy();

    expect(developerPane().getAttribute("aria-checked")).toBe("true");
    fireEvent.click(developerPane());
    expect(workbench()).toBeNull();
    expect(developerPane().getAttribute("aria-checked")).toBe("false");
  });

  // verifies: FLOW-READ-RAW-LOG-R01
  it("closes from the pane's Close, and keyboard focus that was inside goes to the arrange button", async () => {
    await start(fakeCore());
    fireEvent.click(developerPane());
    const close = within(workbench()!).getByRole("button", { name: "Close" });
    close.focus();
    fireEvent.click(close);
    expect(workbench()).toBeNull();
    expect(document.activeElement).toBe(arrangeButton());
  });

  // verifies: FLOW-READ-RAW-LOG-R01
  it("leaves keyboard focus where it is when the workbench closes with focus outside it", async () => {
    await start(fakeCore({ workbench: { open: true } }));
    const close = within(workbench()!).getByRole("button", { name: "Close" });
    const newSession = within(rail()).getByRole("button", { name: "New session" });
    newSession.focus();
    fireEvent.click(close);
    expect(workbench()).toBeNull();
    expect(document.activeElement).toBe(newSession);
  });

  // verifies: FLOW-READ-RAW-LOG-R01
  it("stays while the sessions page shows", async () => {
    await start(fakeCore());
    fireEvent.click(developerPane());
    fireEvent.click(arrangeButton());
    fireEvent.click(screen.getByRole("menuitem", { name: "View all" }));
    expect(document.querySelector(".sessions-page")).not.toBeNull();
    expect(workbench()).not.toBeNull();
  });
});

describe("keeping the workbench across restarts", () => {
  // verifies: FLOW-READ-RAW-LOG-R02
  it("starts closed when nothing is kept, and writes each opening and closing to Core's settings", async () => {
    const core = fakeCore();
    await start(core);
    expect(workbench()).toBeNull();
    fireEvent.click(developerPane());
    fireEvent.click(developerPane());
    expect(core.writes()).toEqual([{ workbench: { open: true } }, { workbench: { open: false } }]);
  });

  // verifies: FLOW-READ-RAW-LOG-R02
  it("starts open, at the width kept, when Ersa starts again", async () => {
    const core = fakeCore();
    const first = await start(core);
    fireEvent.click(developerPane());
    fireEvent.keyDown(edge(), { key: "ArrowLeft" });
    expect(core.settings()).toEqual({ workbench: { open: true, width: 496 } });
    first.unmount();

    await start(core);
    expect(workbench()).not.toBeNull();
    expect(workbench()!.style.getPropertyValue("--workbench-width")).toBe("496px");
    expect(edge().getAttribute("aria-valuenow")).toBe("496");
  });

  // verifies: FLOW-READ-RAW-LOG-R02
  it("reads only an open state of true and a positive width, as Core keeps them unchecked", async () => {
    await start(fakeCore({ workbench: { open: "yes", width: 600 } }));
    expect(workbench()).toBeNull();
    cleanup();
    await start(fakeCore({ workbench: { open: true, width: -5 } }));
    expect(workbench()!.style.getPropertyValue("--workbench-width")).toBe("");
    expect(edge().getAttribute("aria-valuenow")).toBe("480");
  });

  // verifies: FLOW-READ-RAW-LOG-R02
  it("leaves the workbench as the user set it, saying nothing, when Core refuses to keep it, and after Ersa reconnects", async () => {
    const core = fakeCore({}, { refuse: true });
    await start(core);
    fireEvent.click(developerPane());
    fireEvent.keyDown(edge(), { key: "ArrowLeft" });
    await settle();
    expect(core.writes()).toEqual([{ workbench: { open: true } }, { workbench: { width: 496 } }]);
    expect(workbench()).not.toBeNull();
    expect(workbench()!.style.getPropertyValue("--workbench-width")).toBe("496px");
    expect(screen.queryByRole("alert")).toBeNull();
    expect(rail().textContent).not.toContain("a replay keeps no settings");

    // Core's settings, read again on reconnecting, still hold nothing: what the user set stays.
    expect(core.reads()).toBe(1);
    core.drop();
    // Waits on the read the new connection makes, then on its answer being applied.
    await vi.waitFor(() => expect(core.reads()).toBe(2), { timeout: 10_000 });
    await settle();
    await settle();
    expect(workbench()).not.toBeNull();
    expect(workbench()!.style.getPropertyValue("--workbench-width")).toBe("496px");
  });
});

describe("resizing the workbench", () => {
  // verifies: FLOW-READ-RAW-LOG-R16
  it("tells assistive technology the width drawn once the window resizes", async () => {
    await start(fakeCore({ workbench: { open: true, width: 600 } }));
    expect(edge().getAttribute("aria-valuenow")).toBe("600");
    shared = 800;
    act(() => {
      window.dispatchEvent(new Event("resize"));
    });
    expect(edge().getAttribute("aria-valuenow")).toBe("400");
    expect(edge().getAttribute("aria-valuemax")).toBe("400");
  });

  const width = () => Number(edge().getAttribute("aria-valuenow"));

  // verifies: FLOW-READ-RAW-LOG-R16
  it("moves its edge by space-lg with Left and Right, and to its least and greatest width with Home and End", async () => {
    const core = fakeCore();
    await start(core);
    fireEvent.click(developerPane());
    edge().focus();
    expect(width()).toBe(480);
    expect(edge().getAttribute("aria-valuemin")).toBe("320");
    expect(edge().getAttribute("aria-valuemax")).toBe("600");
    fireEvent.keyDown(edge(), { key: "ArrowLeft" });
    expect(width()).toBe(496);
    fireEvent.keyDown(edge(), { key: "ArrowRight" });
    fireEvent.keyDown(edge(), { key: "ArrowRight" });
    expect(width()).toBe(464);
    fireEvent.keyDown(edge(), { key: "End" });
    expect(width()).toBe(600);
    expect(workbench()!.style.getPropertyValue("--workbench-width")).toBe("600px");
    fireEvent.keyDown(edge(), { key: "Home" });
    expect(width()).toBe(320);
    expect(core.writes().slice(1)).toEqual([496, 480, 464, 600, 320].map((w) => ({ workbench: { width: w } })));
  });

  // verifies: FLOW-READ-RAW-LOG-R16
  it("never takes more than half the width it shares with the transcript, nor less than its least while half allows it", async () => {
    await start(fakeCore());
    fireEvent.click(developerPane());
    fireEvent.keyDown(edge(), { key: "End" });
    fireEvent.keyDown(edge(), { key: "ArrowLeft" });
    expect(width()).toBe(600);
    fireEvent.keyDown(edge(), { key: "Home" });
    fireEvent.keyDown(edge(), { key: "ArrowRight" });
    expect(width()).toBe(320);
    // Half the shared width is less than the least width: half is both its least and its greatest.
    shared = 500;
    fireEvent.keyDown(edge(), { key: "Home" });
    expect(width()).toBe(250);
    fireEvent.keyDown(edge(), { key: "ArrowLeft" });
    expect(width()).toBe(250);
    expect(edge().getAttribute("aria-valuemin")).toBe("250");
    expect(edge().getAttribute("aria-valuemax")).toBe("250");
  });

  // verifies: FLOW-READ-RAW-LOG-R16
  it("starts a step from the width shown when the kept width is more than the window allows", async () => {
    await start(fakeCore({ workbench: { open: true, width: 900 } }));
    fireEvent.keyDown(edge(), { key: "ArrowRight" });
    expect(width()).toBe(584);
  });

  // verifies: FLOW-READ-RAW-LOG-R16
  it("follows a drag of its edge, within its limits, and keeps the width once the drag ends", async () => {
    const core = fakeCore();
    await start(core);
    fireEvent.click(developerPane());
    fireEvent.pointerDown(edge(), { button: 0, pointerId: 1, clientX: 700 });
    fireEvent.pointerMove(edge(), { pointerId: 1, clientX: 650 });
    expect(width()).toBe(530);
    expect(workbench()!.style.getPropertyValue("--workbench-width")).toBe("530px");
    fireEvent.pointerMove(edge(), { pointerId: 1, clientX: 100 });
    expect(width()).toBe(600);
    expect(core.writes()).toEqual([{ workbench: { open: true } }]);
    fireEvent.pointerUp(edge(), { pointerId: 1, clientX: 100 });
    expect(core.writes()).toEqual([{ workbench: { open: true } }, { workbench: { width: 600 } }]);
  });
});
