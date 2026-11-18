// @vitest-environment jsdom
// The unseen mark in the window, against a fake Core that stores it as Core does (PROTO-CORE-API-R78): Mark as unread
// and Mark as read on a tile's menu (FLOW-ORGANIZE-SESSIONS-R24), Ersa's own mark on a session that goes idle after
// working while not active, and both surviving a restart of Ersa and of Core (R25).

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, CoreEvent, Method } from "./core/client";
import type { CoreHelloResult, Session, Tile } from "./protocol/core-api.gen";

afterEach(cleanup);

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const session = (id: string, title: string, created_at: string): Session => ({
  id,
  title,
  workspace_id: "w-alpha",
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  state: "idle",
  created_at,
  archived: false,
  pinned: false,
  unseen: false,
  tile: { state: "idle", pending: [] },
});

/**
 * A Core whose sessions outlive any window: `session.arrange` stores `unseen` and the list reports it, so a new App on
 * the same fake is Ersa restarted, and a drop and attach again is Core restarted. s2 is the newest, so the active
 * session on opening. `refuse` makes every arrange fail, or the first so many; `unseen` lists sessions Core already
 * has unseen.
 */
function fakeCore({ refuse = false as boolean | number, unseen = [] as string[] } = {}) {
  let list = [session("s1", "Fix the build", "2026-11-01T09:00:00Z"), session("s2", "Write the notes", "2026-11-02T09:00:00Z")].map((s) => ({
    ...s,
    unseen: unseen.includes(s.id),
  }));
  let refusals = refuse === true ? Infinity : refuse === false ? 0 : refuse;
  let onEvent: (event: CoreEvent) => void = () => {};
  // How many times the window has read the list: a reconnect reads it again.
  let reads = 0;
  const arranges: unknown[] = [];
  const answers: Partial<Record<Method, (params: never) => unknown>> = {
    "core.status": () => ({ sessions_running: 0, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => [{ id: "alpha", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true }],
    "session.list": () => {
      reads++;
      return list;
    },
    "workspace.list": () => [{ id: "w-alpha", project_id: "alpha", kind: "root", path: "D:/code/alpha", vcs_ref: "main", state: "ready" }],
    "backend.list": () => [],
    "settings.get": () => ({}),
    "session.subscribe": () => ({ view: "v1", last_seq: 0 }),
    "session.unsubscribe": () => ({}),
    "session.arrange": (params: { id: string; unseen?: boolean }) => {
      arranges.push(params);
      if (refusals-- > 0) throw { kind: "unavailable", message: "the database is locked" };
      list = list.map((s) => (s.id === params.id && params.unseen !== undefined ? { ...s, unseen: params.unseen } : s));
      return list.find((s) => s.id === params.id);
    },
  };
  const client: CoreClient = {
    attach: (listener) => {
      onEvent = listener;
      return Promise.resolve<Attachment>({ how: "attached", hello });
    },
    call: (method, params) => {
      const answer = answers[method];
      if (!answer) return Promise.reject({ kind: "unavailable", message: `no answer for ${method}` }) as never;
      return new Promise((resolve) => resolve(answer(params as never))) as never;
    },
  };
  // A session's tile changes as Core reports it, its title kept.
  // `tokens` makes each tile differ, as a working session's do.
  const tile = (id: string, state: Tile["state"], tokens?: number) => {
    list = list.map((s) => (s.id === id ? { ...s, state, tile: { state, pending: [], context_tokens: tokens } } : s));
    const s = list.find((x) => x.id === id) as Session;
    return act(() =>
      onEvent({ kind: "notification", method: "session.tile", params: { session_id: id, title: s.title, created_at: s.created_at, tile: s.tile } } as CoreEvent),
    );
  };
  // A session's state changes in Core with no tile sent, as while the window can't reach it.
  // `gone` takes it off the list.
  const quietly = (id: string, state: Tile["state"] | "gone") => {
    list = state === "gone" ? list.filter((s) => s.id !== id) : list.map((s) => (s.id === id ? { ...s, state, tile: { state, pending: [] } } : s));
  };
  return { client, arranges, tile, quietly, reads: () => reads, drop: () => act(() => onEvent({ kind: "lost", reason: "gone" })), unseen: (id: string) => list.find((s) => s.id === id)?.unseen };
}

const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));
const rail = () => screen.getByRole("navigation", { name: "Rail" });
const tileOf = (title: string) => [...rail().querySelectorAll<HTMLElement>(".session")].find((el) => el.textContent?.includes(title)) as HTMLElement;
/** Whether the tile reads unseen: its title at the heading weight. */
const reads = (title: string) => tileOf(title).querySelector(".session-title")?.hasAttribute("data-unseen");
const menuOf = (title: string) => fireEvent.click(screen.getByRole("button", { name: `More for ${title}` }));
const item = (name: RegExp) => screen.getByRole("menuitem", { name });

async function open(core: ReturnType<typeof fakeCore>) {
  render(createElement(App, { client: core.client }));
  await settle();
  await settle();
}

/** Ersa restarted: the window closes and a new one opens on the same Core. */
async function restartErsa(core: ReturnType<typeof fakeCore>) {
  cleanup();
  await open(core);
}

describe("Mark as unread and Mark as read on a tile's menu", () => {
  // verifies: FLOW-ORGANIZE-SESSIONS-R24
  it("offers Mark as unread after Pin on a session that isn't unseen, and Mark as read once it is", async () => {
    const core = fakeCore();
    await open(core);
    menuOf("Fix the build");
    const names = screen.getAllByRole("menuitem").map((el) => el.textContent);
    expect(names.slice(0, 2)).toEqual(["Pin", "Mark as unread"]);
    fireEvent.click(item(/^Mark as unread/));
    await settle();
    expect(core.arranges).toEqual([{ id: "s1", unseen: true }]);
    expect(reads("Fix the build")).toBe(true);

    menuOf("Fix the build");
    expect(screen.queryByRole("menuitem", { name: /^Mark as unread/ })).toBeNull();
    fireEvent.click(item(/^Mark as read/));
    await settle();
    expect(core.arranges).toEqual([
      { id: "s1", unseen: true },
      { id: "s1", unseen: false },
    ]);
    expect(reads("Fix the build")).toBe(false);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R24
  it("shows Mark as unread unavailable, with the reason, on the active session", async () => {
    const core = fakeCore();
    await open(core);
    expect(tileOf("Write the notes").getAttribute("aria-current")).toBe("true");
    menuOf("Write the notes");
    const unread = item(/^Mark as unread/);
    expect(unread.getAttribute("aria-disabled")).toBe("true");
    expect(unread.textContent).toBe("Mark as unreadAvailable once you open another session");
    fireEvent.click(unread);
    await settle();
    expect(core.arranges).toEqual([]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R24
  it("shows it unavailable on the sessions page's row for the session behind the page, which stays active", async () => {
    const core = fakeCore();
    await open(core);
    fireEvent.click(screen.getByRole("button", { name: "Arrange sessions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "View all" }));
    await settle();
    const row = (title: string) =>
      [...document.querySelectorAll<HTMLElement>(".sessions-page-row")].find((el) => el.textContent?.includes(title)) as HTMLElement;
    const unreadOn = (title: string) => {
      fireEvent.click(within(row(title)).getByRole("button", { name: `More for ${title}` }));
      return within(screen.getByRole("menu", { name: `More for ${title}` })).getByRole("menuitem", { name: /^Mark as unread/ });
    };
    expect(unreadOn("Write the notes").getAttribute("aria-disabled")).toBe("true");
    fireEvent.keyDown(screen.getByRole("menu", { name: "More for Write the notes" }), { key: "Escape" });
    expect(unreadOn("Fix the build").getAttribute("aria-disabled")).toBeNull();
  });

  it("says why beneath the tile when Core refuses, the tile as Core has it", async () => {
    const core = fakeCore({ refuse: true });
    await open(core);
    menuOf("Fix the build");
    fireEvent.click(item(/^Mark as unread/));
    await settle();
    expect(reads("Fix the build")).toBe(false);
    expect(rail().textContent).toContain("Ersa can't mark this session as unread.");
    expect(rail().textContent).toContain("the database is locked");
  });
});

describe("the unseen mark across restarts", () => {
  // verifies: FLOW-ORGANIZE-SESSIONS-R25
  // verifies: FLOW-ORGANIZE-SESSIONS-R37
  // verifies: FLOW-ORGANIZE-SESSIONS-R39
  it("keeps Ersa's mark, on a session that went idle after working while not active, through a restart of Ersa", async () => {
    const core = fakeCore();
    await open(core);
    await core.tile("s1", "running");
    await settle();
    expect(core.arranges).toEqual([]);
    await core.tile("s1", "idle");
    await settle();
    expect(core.arranges).toEqual([{ id: "s1", unseen: true }]);
    expect(reads("Fix the build")).toBe(true);

    await restartErsa(core);
    expect(reads("Fix the build")).toBe(true);
    // Made active, it is seen, in Core too, so it reads as seen after the next restart.
    fireEvent.click(tileOf("Fix the build"));
    expect(reads("Fix the build")).toBe(false);
    await settle();
    expect(core.unseen("s1")).toBe(false);
    await restartErsa(core);
    expect(reads("Fix the build")).toBe(false);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R25
  it("keeps the user's mark through a restart of Core", async () => {
    const core = fakeCore();
    await open(core);
    menuOf("Fix the build");
    fireEvent.click(item(/^Mark as unread/));
    await settle();
    await core.drop();
    // The window attaches again to Core, which reads its sessions afresh.
    await waitFor(() => expect(rail().textContent).toContain("Fix the build"), { timeout: 5_000 });
    expect(core.unseen("s1")).toBe(true);
    expect(reads("Fix the build")).toBe(true);
  });

  it("sends the mark again once Core takes it, after Core refused it", async () => {
    const core = fakeCore({ refuse: 1, unseen: ["s1"] });
    await open(core);
    fireEvent.click(tileOf("Fix the build"));
    await settle();
    // Refused: Core still has it unseen, and the tile says why.
    expect(core.unseen("s1")).toBe(true);
    expect(rail().textContent).toContain("Ersa can't mark this session as read.");
    // Left and opened again, it is marked seen, and the refusal goes.
    fireEvent.click(tileOf("Write the notes"));
    await settle();
    fireEvent.click(tileOf("Fix the build"));
    await settle();
    expect(core.unseen("s1")).toBe(false);
    expect(rail().textContent).not.toContain("Ersa can't mark this session");
  });

  it("asks again only as the session starts working, not on every tile while it works, when Core keeps refusing", async () => {
    const core = fakeCore({ refuse: true, unseen: ["s1"] });
    await open(core);
    fireEvent.click(tileOf("Fix the build"));
    await settle();
    for (const tokens of [1, 2, 3, 4]) {
      await core.tile("s1", "running", tokens * 1000);
      await settle();
    }
    expect(core.arranges).toEqual([
      { id: "s1", unseen: false },
      { id: "s1", unseen: false },
    ]);
  });

  it("asks again once Ersa reaches Core again after losing it", async () => {
    const core = fakeCore({ refuse: 1, unseen: ["s1"] });
    await open(core);
    fireEvent.click(tileOf("Fix the build"));
    await settle();
    expect(core.unseen("s1")).toBe(true);
    await core.drop();
    await waitFor(() => expect(core.unseen("s1")).toBe(false), { timeout: 5_000 });
    expect(core.arranges).toHaveLength(2);
  });

  /** Core drops while `id` is working, its state changes to `state` meanwhile, and the window attaches again. */
  async function reconnectAfter(core: ReturnType<typeof fakeCore>, id: string, state: Tile["state"] | "gone") {
    await core.tile(id, "running");
    const before = core.reads();
    await core.drop();
    core.quietly(id, state);
    await waitFor(() => expect(core.reads()).toBeGreaterThan(before), { timeout: 5_000 });
    await settle();
  }

  // verifies: FLOW-ORGANIZE-SESSIONS-R38
  it("marks a session whose turn ended while Ersa couldn't reach Core, once connected again", async () => {
    const core = fakeCore();
    await open(core);
    await reconnectAfter(core, "s1", "idle");
    expect(core.arranges).toEqual([{ id: "s1", unseen: true }]);
    expect(reads("Fix the build")).toBe(true);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R38
  it.each(["ended", "error", "disconnected", "gone"] as const)("marks nothing on reconnecting for a session that is %s meanwhile", async (state) => {
    const core = fakeCore();
    await open(core);
    await reconnectAfter(core, "s1", state);
    expect(core.arranges).toEqual([]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R38
  it("marks nothing on reconnecting for the active session, whose turn ended meanwhile", async () => {
    const core = fakeCore();
    await open(core);
    await reconnectAfter(core, "s2", "idle");
    expect(core.arranges).toEqual([]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R37
  it("never marks the active session, though it goes idle after working", async () => {
    const core = fakeCore();
    await open(core);
    await core.tile("s2", "running");
    await core.tile("s2", "idle");
    await settle();
    expect(core.arranges).toEqual([]);
  });
});
