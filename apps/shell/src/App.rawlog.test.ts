// @vitest-environment jsdom
// The developer pane in the window against a fake Core (FLOW-READ-RAW-LOG, steps 2 and 3): the active session's raw
// log listed as Core holds it, following the end as lines arrive, and what the pane says with no session or before
// the first answer. The list's own behavior is in components/RawLogList.dom.test.ts.

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, Method } from "./core/client";
import type { ShellControl } from "./core/shell";
import type { BackendInfo, CoreHelloResult, RawLogEntry, Session, SessionRawLogParams } from "./protocol/core-api.gen";

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const claudeCode = {
  name: "claude-code",
  label: "Claude Code",
  version: "2.1.316",
  status: "available",
  models: [],
  efforts: [],
  permission_modes: [],
} as unknown as BackendInfo;

const session: Session = {
  id: "s1",
  title: "Fix the build",
  workspace_id: "w1",
  host_id: "local",
  backend: "claude-code",
  model: "opus",
  effort: "low",
  permission_mode: "default",
  state: "idle",
  created_at: "2026-11-01T09:00:00Z",
  archived: false,
  pinned: false,
  unseen: false,
  tile: { state: "idle", pending: [] },
};

const entry = (index: number, dir: "backend" | "host" = "backend"): RawLogEntry => ({ index, ts: "2026-11-22T10:00:00.000Z", dir, line: `line ${index}` });

type Answers = Partial<Record<Method, (params: never) => unknown>>;

/**
 * A Core with one session, the workbench kept open, and a raw log the test adds lines to. While `hold` is set, raw
 * log reads wait until it is cleared.
 */
function fakeCore(label?: string) {
  const held: RawLogEntry[] = [];
  let release: (() => void) | undefined;
  let holding = false;
  const answers: Answers = {
    "core.status": () => ({ sessions_running: 1, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => [{ id: "alpha", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true }],
    "session.list": () => [session],
    "workspace.list": () => [{ id: "w1", project_id: "alpha", kind: "isolated", path: "D:/ws/alpha-1", vcs_ref: "ersa/fix", state: "ready" }],
    "backend.list": () => [claudeCode],
    "section.list": () => [],
    "settings.get": () => ({ workbench: { open: true } }),
    "settings.set": () => ({}),
    "session.subscribe": () => ({ view: "v1", last_seq: 0 }),
    "session.unsubscribe": () => ({}),
    "session.raw_log": (params: SessionRawLogParams) => held.filter((e) => e.index >= (params.from ?? 0)),
  };
  const client: CoreClient = {
    attach: () => Promise.resolve<Attachment>({ how: "attached", hello, ...(label === undefined ? {} : { label }) }),
    call: (method, params) => {
      const answer = answers[method];
      if (!answer) return Promise.reject({ kind: "unavailable", message: `no answer for ${method}` }) as never;
      const wait = method === "session.raw_log" && holding ? new Promise<void>((r) => (release = r)) : Promise.resolve();
      return wait.then(() => answer(params as never)) as never;
    },
  };
  return {
    client,
    add: (...entries: RawLogEntry[]) => held.push(...entries),
    hold: () => (holding = true),
    let: () => {
      holding = false;
      act(() => release?.());
    },
  };
}

const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));
async function start(core: ReturnType<typeof fakeCore>, shell?: ShellControl) {
  render(createElement(App, { client: core.client, shell }));
  for (let i = 0; i < 4; i++) await settle();
}

const ROW = 24;
const HEIGHT = 240;
beforeEach(() => {
  document.documentElement.style.setProperty("--size-log-row", `${ROW}px`);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(function (this: HTMLElement) {
    return this.classList.contains("raw-log-list") ? HEIGHT : 0;
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const pane = () => screen.getByRole("complementary", { name: "Developer" });
const list = () => within(pane()).getByRole("listbox", { name: "Raw log" });
const rows = () => within(list()).queryAllByRole("option");

describe("the developer pane in the window", () => {
  // verifies: FLOW-READ-RAW-LOG-R03
  it("lists the active session's raw log, each direction in words with the backend's own label", async () => {
    const core = fakeCore();
    core.add(entry(0, "host"), entry(1, "backend"));
    await start(core);
    await waitFor(() => expect(rows()).toHaveLength(2));
    const [first, second] = rows();
    expect(first?.textContent).toContain("line 0");
    expect(within(first!).getByRole("img").getAttribute("aria-label")).toBe("To Claude Code");
    expect(within(second!).getByRole("img").getAttribute("aria-label")).toBe("From Claude Code");
  });

  // verifies: FLOW-READ-RAW-LOG-R07
  it("follows the end as the session's lines arrive", async () => {
    const core = fakeCore();
    core.add(...Array.from({ length: 30 }, (_, i) => entry(i)));
    await start(core);
    await waitFor(() => expect(list().scrollTop).toBe(30 * ROW - HEIGHT));
    core.add(entry(30), entry(31));
    // The reader asks again within its second; the list is at the new end once it has them.
    await waitFor(() => expect(list().scrollTop).toBe(32 * ROW - HEIGHT), { timeout: 5_000 });
  });

  // verifies: FLOW-READ-RAW-LOG-R14
  it("says it is reading until the first answer comes, then that lines appear while the session runs", async () => {
    const core = fakeCore();
    core.hold();
    await start(core);
    expect(within(pane()).getByText("Reading…")).toBeTruthy();
    core.let();
    await waitFor(() => expect(within(pane()).getByText("No lines yet. Lines appear here while the session runs.")).toBeTruthy());
  });

  // verifies: FLOW-READ-RAW-LOG-R12
  it("filters the session's lines as they arrive, and counts them", async () => {
    const core = fakeCore();
    core.add(entry(0), entry(1), entry(10));
    await start(core);
    await waitFor(() => expect(rows()).toHaveLength(3));
    fireEvent.change(within(pane()).getByRole("searchbox", { name: "Filter lines" }), { target: { value: "LINE 1" } });
    expect(rows().map((row) => row.textContent)).toEqual([expect.stringContaining("line 1"), expect.stringContaining("line 10")]);
    core.add(entry(11), entry(12), entry(20));
    // The reader asks again within its second; the new lines that contain the text join the list.
    await waitFor(() => expect(rows()).toHaveLength(4), { timeout: 4_000 });
    expect(within(pane()).getByText("4 of 6")).toBeTruthy();
  });

  // verifies: FLOW-READ-RAW-LOG-R10
  it("shows the selected line in the detail beneath the list", async () => {
    const core = fakeCore();
    core.add(entry(0), entry(1));
    await start(core);
    await waitFor(() => expect(rows()).toHaveLength(2));
    fireEvent.click(rows()[1]!);
    const detail = pane().querySelector(".raw-log-detail")!;
    expect(detail.hasAttribute("data-shown")).toBe(true);
    expect(detail.querySelector("pre")?.textContent).toBe("line 1");
    expect(within(detail as HTMLElement).getByRole("button", { name: "Copy line" })).toBeTruthy();
  });

  // verifies: FLOW-READ-RAW-LOG-R14
  it("says to choose a session while the new-session screen shows", async () => {
    const core = fakeCore();
    await start(core);
    fireEvent.click(within(screen.getByRole("navigation", { name: "Rail" })).getByRole("button", { name: "New session" }));
    await settle();
    expect(within(pane()).getByText("No session is open. Choose a session to see its raw log.")).toBeTruthy();
  });
});

describe("Open diagnostic log in the window", () => {
  const shellWith = (openDiagnosticLog: () => Promise<void>) =>
    ({
      onQuitRequested: () => () => {},
      quitHeard: async () => {},
      show: async () => false,
      hide: async () => {},
      quit: async () => {},
      pickFolder: async () => null,
      pickFiles: async () => [],
      onDragDrop: () => () => {},
      openLink: () => {},
      openDiagnosticLog,
    }) satisfies ShellControl;
  const button = () => within(pane()).getByRole("button", { name: "Open diagnostic log" });

  // verifies: FLOW-READ-RAW-LOG-R13
  it("reaches the Shell's command, and says why when the Shell couldn't open the file", async () => {
    const open = vi.fn<() => Promise<void>>(() => Promise.reject("There's no diagnostic log at D:\\data\\core.log yet."));
    await start(fakeCore(), shellWith(open));
    expect(button().getAttribute("aria-disabled")).toBeNull();
    fireEvent.click(button());
    await settle();
    expect(open).toHaveBeenCalledTimes(1);
    expect(within(pane()).getAllByRole("status").map((s) => s.textContent)).toContain("Ersa couldn't open the diagnostic log. There's no diagnostic log at D:\\data\\core.log yet.");
  });

  // verifies: FLOW-READ-RAW-LOG-R13
  it("is unavailable in a debug build attached to a labeled endpoint, naming it", async () => {
    const open = vi.fn(async () => {});
    await start(fakeCore("replay-3"), shellWith(open));
    expect(button().getAttribute("aria-disabled")).toBe("true");
    expect(button().getAttribute("title")).toBe("This build is attached to replay-3, so it has no diagnostic log to open.");
    fireEvent.click(button());
    await settle();
    expect(open).not.toHaveBeenCalled();
  });
});
