// @vitest-environment jsdom
// The sessions page in the window (FLOW-ORGANIZE-SESSIONS, step 9): All sessions in the arrange menu, what the page
// lists and hides, choosing a row against a fake Core, and a row's menu being the tile's.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, Method } from "./core/client";
import type { ShellControl } from "./core/shell";
import type { CoreHelloResult, Session, Workspace } from "./protocol/core-api.gen";

afterEach(cleanup);

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const workspaces: Workspace[] = [{ id: "w1", project_id: "alpha", kind: "isolated", path: "D:/ws/alpha-1", vcs_ref: "ersa/fix", state: "ready" }];

const session = (id: string, title: string, created_at: string, more: Partial<Session> = {}): Session => ({
  id,
  title,
  workspace_id: "w1",
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  state: "idle",
  created_at,
  archived: false,
  pinned: false,
  tile: { state: "idle", pending: [] },
  ...more,
});

// Newest first by last activity: s2, s3, s1. s3 is the newest created, so it is the active session on opening.
const listed = [
  session("s1", "Fix the build", "2026-11-01T09:00:00Z"),
  session("s2", "Write the notes", "2026-10-01T09:00:00Z", { last_activity_at: "2026-11-05T09:00:00Z" }),
  session("s3", "Tidy the docs", "2026-11-03T09:00:00Z"),
  session("s4", "Retire the protocol", "2026-11-02T09:00:00Z", { archived: true }),
];

function fakeCore(more: Partial<Record<Method, (params: never) => unknown>> = {}) {
  let list = listed;
  const calls: { method: Method; params: unknown }[] = [];
  const answers: Partial<Record<Method, (params: never) => unknown>> = {
    "core.status": () => ({ sessions_running: 0, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => [{ id: "alpha", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true }],
    "session.list": () => list,
    "workspace.list": () => workspaces,
    "backend.list": () => [],
    "settings.get": () => ({}),
    "session.subscribe": () => ({ view: "v1", last_seq: 0 }),
    "session.unsubscribe": () => ({}),
    "session.set_archived": (params: { id: string; archived: boolean }) => {
      list = list.map((s) => (s.id === params.id ? { ...s, archived: params.archived } : s));
      return {};
    },
    ...more,
  };
  const client: CoreClient = {
    attach: () => Promise.resolve<Attachment>({ how: "attached", hello }),
    call: (method, params) => {
      calls.push({ method, params });
      const answer = answers[method];
      if (!answer) return Promise.reject({ kind: "unavailable", message: `no answer for ${method}` }) as never;
      return new Promise((resolve) => resolve(answer(params as never))) as never;
    },
  };
  return { client, calls };
}

const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));
const rail = () => screen.getByRole("navigation", { name: "Rail" });
const page = () => document.querySelector<HTMLElement>(".sessions-page");
const titles = () => [...(page()?.querySelectorAll(".sessions-page-title") ?? [])].map((el) => el.textContent);
const row = (title: string) => [...(page()?.querySelectorAll<HTMLElement>(".sessions-page-row") ?? [])].find((el) => el.textContent?.includes(title));
const open = (title: string) => row(title)?.querySelector<HTMLElement>(".session") as HTMLElement;

function fakeShell() {
  const stopped = vi.fn();
  return {
    stopped,
    onQuitRequested: () => () => {},
    quitHeard: async () => {},
    show: async () => false,
    hide: async () => {},
    quit: async () => {},
    pickFolder: vi.fn(async () => null),
    pickFiles: async () => [] as string[],
    onDragDrop: () => stopped,
  } satisfies ShellControl & { stopped: unknown };
}

async function start(more: Parameters<typeof fakeCore>[0] = {}, shell?: ShellControl) {
  const core = fakeCore(more);
  render(createElement(App, { client: core.client, shell }));
  await settle();
  await settle();
  return core;
}

async function allSessions() {
  fireEvent.click(within(rail()).getByRole("button", { name: "Arrange sessions" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "All sessions" }));
  await settle();
}

describe("the sessions page", () => {
  // verifies: FLOW-ORGANIZE-SESSIONS-R16
  it("opens from the arrange menu listing every session for its tab, Active first, whatever the rail's arrangement", async () => {
    // The rail shows archived sessions only, with its one group folded: none of that reaches the page.
    await start({ "settings.get": () => ({ rail: { show: "archived", group_by: "project", collapsed: { project: ["alpha"] } } }) });
    expect([...rail().querySelectorAll(".session")].map((el) => el.textContent)).toEqual([expect.stringContaining("Retire the protocol")]);
    await allSessions();
    expect(screen.getByRole("heading", { name: "Sessions" })).toBeTruthy();
    expect(screen.getByRole("tab", { name: "Active" }).getAttribute("aria-selected")).toBe("true");
    expect(titles()).toEqual(["Write the notes", "Tidy the docs", "Fix the build"]);
    fireEvent.click(screen.getByRole("tab", { name: "Archived" }));
    expect(titles()).toEqual(["Retire the protocol"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R17
  it("searches titles ignoring case, and Escape clears and closes the field, focus back on its button", async () => {
    await start();
    await allSessions();
    fireEvent.click(screen.getByRole("button", { name: "Search sessions" }));
    const field = screen.getByRole("searchbox", { name: "Search sessions" });
    expect(document.activeElement).toBe(field);
    fireEvent.change(field, { target: { value: "WRITE" } });
    expect(titles()).toEqual(["Write the notes"]);
    fireEvent.change(field, { target: { value: "nothing like it" } });
    expect(page()?.textContent).toContain("No sessions match your search.");
    fireEvent.keyDown(field, { key: "Escape" });
    expect(screen.queryByRole("searchbox")).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Search sessions" }));
    expect(titles()).toHaveLength(3);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R18
  it("makes a chosen session active and shows its transcript in place of the page", async () => {
    const core = await start();
    await allSessions();
    fireEvent.click(open("Fix the build"));
    await settle();
    expect(page()).toBeNull();
    expect(core.calls.some((c) => c.method === "session.subscribe" && (c.params as { id: string }).id === "s1")).toBe(true);
    expect(screen.getByRole("heading", { name: "Fix the build" })).toBeTruthy();
  });

  it("leaves when a tile or New session is chosen in the rail", async () => {
    await start();
    await allSessions();
    fireEvent.click(within(rail()).getByRole("button", { name: "New session" }));
    await settle();
    expect(page()).toBeNull();
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R19
  it("says so while the sessions are being read, not that there are none", async () => {
    await start({ "session.list": () => new Promise(() => {}) });
    await allSessions();
    expect(page()?.textContent).toContain("Loading…");
    expect(page()?.textContent).not.toContain("No active sessions.");
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R19
  it("says so, with the reason, when the sessions can't be read", async () => {
    await start({ "session.list": () => Promise.reject({ kind: "unavailable", message: "the database is locked" }) });
    await allSessions();
    const list = page()?.querySelector(".sessions-page-list");
    expect(list?.textContent).toContain("Ersa can't read your sessions.");
    expect(list?.textContent).toContain("the database is locked");
    expect(list?.textContent).not.toContain("No active sessions.");
  });

  it("says an empty tab has nothing to list", async () => {
    await start({ "session.list": () => [] });
    await allSessions();
    expect(page()?.textContent).toContain("No active sessions.");
    fireEvent.click(screen.getByRole("tab", { name: "Archived" }));
    expect(page()?.textContent).toContain("No archived sessions.");
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R20
  it("hides the composer, the controls and the header, and keeps the active session active with its tile highlighted", async () => {
    await start();
    expect(screen.getByRole("textbox", { name: /message/i })).toBeTruthy();
    await allSessions();
    expect(screen.queryByRole("textbox", { name: /message/i })).toBeNull();
    expect(screen.queryByRole("heading", { name: "Tidy the docs" })).toBeNull();
    expect(document.querySelector("main:not([hidden]) .session-controls")).toBeNull();
    expect(document.querySelector("main[hidden]")?.hasAttribute("inert")).toBe(true);
    const highlighted = rail().querySelector('.session[aria-current="true"]');
    expect(highlighted?.textContent).toContain("Tidy the docs");
  });

  it("names each row's project and last activity, and the same menu a tile has", async () => {
    await start();
    await allSessions();
    expect(row("Fix the build")?.textContent).toContain("alpha");
    expect(row("Fix the build")?.textContent).toContain("Nov 1, 2026");
    const names = (scope: HTMLElement, title: string) => {
      fireEvent.click(within(scope).getByRole("button", { name: `More for ${title}` }));
      const items = screen.getAllByRole("menuitem").map((el) => el.textContent);
      fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
      return items;
    };
    expect(names(row("Fix the build") as HTMLElement, "Fix the build")).toEqual(["Pin", "RenameF2", "Fork", "Archive", "Delete…"]);
  });

  it("shows the mark a tile shows, named, and the host when it is not this machine", async () => {
    await start({
      "session.list": () => [
        session("n", "Needs an answer", "2026-11-01T09:00:00Z", { tile: { state: "waiting_user", pending: [{ kind: "permission", request_id: "r1" }] } }),
        session("e", "Went wrong", "2026-11-01T08:00:00Z", { tile: { state: "error", pending: [] }, host_id: "devbox" }),
      ],
    });
    await allSessions();
    expect(within(row("Needs an answer") as HTMLElement).getByRole("img", { name: "Needs permission" })).toBeTruthy();
    expect(within(row("Went wrong") as HTMLElement).getByRole("img", { name: "Error" })).toBeTruthy();
    expect(row("Went wrong")?.querySelector(".sessions-page-facts")?.textContent).toContain("devbox");
    expect(row("Needs an answer")?.querySelector(".sessions-page-facts")?.textContent).not.toContain("local");
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R20
  it("really hides what stands behind the page, whose own display would otherwise show it", async () => {
    const style = document.createElement("style");
    style.textContent = readFileSync(join(import.meta.dirname, "App.css"), "utf8");
    document.head.append(style);
    try {
      await start();
      await allSessions();
      expect(getComputedStyle(document.querySelector("main[hidden]") as Element).visibility).toBe("hidden");
    } finally {
      style.remove();
    }
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R20
  it("takes no Ctrl+O for the new-session screen behind the page", async () => {
    const shell = fakeShell();
    await start({ "session.list": () => [] }, shell);
    await allSessions();
    fireEvent.keyDown(document, { key: "o", ctrlKey: true });
    await settle();
    expect(shell.pickFolder).not.toHaveBeenCalled();
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R20
  it("stops taking dropped files for the composer behind the page", async () => {
    const shell = fakeShell();
    await start({}, shell);
    await settle();
    // Moving from the new-session screen to the session as the window opens stops one listener already.
    const before = shell.stopped.mock.calls.length;
    await allSessions();
    expect(shell.stopped.mock.calls.length).toBeGreaterThan(before);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R20
  it("keeps a message being written, and gives it focus, when the same session is chosen again", async () => {
    await start();
    fireEvent.change(screen.getByRole("textbox", { name: /message/i }), { target: { value: "half a thought" } });
    await allSessions();
    fireEvent.click(open("Tidy the docs"));
    await settle();
    const box = screen.getByRole<HTMLTextAreaElement>("textbox", { name: /message/i });
    expect(box.value).toBe("half a thought");
    expect(document.activeElement).toBe(box);
  });

  it("leaves a session opened meanwhile alone when an archive from its row answers late", async () => {
    let answer: (value: object) => void = () => {};
    await start({ "session.set_archived": () => new Promise((resolve) => (answer = resolve)) });
    await allSessions();
    fireEvent.click(within(row("Tidy the docs") as HTMLElement).getByRole("button", { name: "More for Tidy the docs" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Archive" }));
    await settle();
    fireEvent.click(open("Fix the build"));
    await settle();
    await act(async () => answer({}));
    await settle();
    expect(screen.getByRole("heading", { name: "Fix the build" })).toBeTruthy();
  });

  it("stays when the active session is archived from its row, the row moving to Archived and focus to the next row", async () => {
    const core = await start();
    await allSessions();
    const home = row("Tidy the docs") as HTMLElement;
    const more = within(home).getByRole("button", { name: "More for Tidy the docs" });
    more.focus();
    fireEvent.click(more);
    fireEvent.click(screen.getByRole("menuitem", { name: "Archive" }));
    await settle();
    await settle();
    expect(core.calls.some((c) => c.method === "session.set_archived")).toBe(true);
    expect(page()).not.toBeNull();
    expect(titles()).toEqual(["Write the notes", "Fix the build"]);
    expect(document.activeElement).toBe(open("Fix the build"));
  });

  it("puts focus somewhere when an archived row opens, having no composer to take it", async () => {
    await start();
    await allSessions();
    fireEvent.click(screen.getByRole("tab", { name: "Archived" }));
    open("Retire the protocol").focus();
    fireEvent.click(open("Retire the protocol"));
    await settle();
    expect(page()).toBeNull();
    expect(document.activeElement).not.toBe(document.body);
  });

  it("gives focus back to the row when the delete question opened from it is canceled", async () => {
    await start({ "workspace.changes": () => ({ uncommitted: 0, ignored: 0, unshared: 0, stranded: 0, sessions: 1, unrecorded: false, token: "t1" }) });
    await allSessions();
    fireEvent.click(within(row("Fix the build") as HTMLElement).getByRole("button", { name: "More for Fix the build" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /^Delete…/ }));
    await settle();
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }));
    await settle();
    expect(document.activeElement).toBe(open("Fix the build"));
  });
});
