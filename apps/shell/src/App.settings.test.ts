// @vitest-environment jsdom
// The settings page in the window (LAY-SHELL, The settings page; FLOW-CLEAN-UP, Storage) against a fake Core:
// Settings in the arrange menu, what the page hides and keeps behind it, and Archive inactive sessions read from and
// written to Core's settings.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, Method } from "./core/client";
import { mergePatch } from "./session/arrangement";
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

const session = (id: string, title: string, created_at: string): Session => ({
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
  unseen: false,
  tile: { state: "idle", pending: [] },
});

// s2 is the newest, so it is the active session on opening.
const listed = [session("s1", "Fix the build", "2026-11-01T09:00:00Z"), session("s2", "Tidy the docs", "2026-11-03T09:00:00Z")];

type Gate = { open: () => void };

/**
 * A Core with two sessions that keeps settings as Core does, each patch merged in. `read` replaces `settings.get`'s
 * answer; `refuse` makes `settings.set` refuse; `hold` makes it wait until the returned gate opens.
 */
function fakeCore(kept: unknown = {}, { read, refuse, hold }: { read?: () => Promise<unknown>; refuse?: boolean; hold?: boolean } = {}) {
  let settings = kept;
  const calls: { method: Method; params: unknown }[] = [];
  const gates: Gate[] = [];
  const answers: Partial<Record<Method, (params: never) => unknown>> = {
    "core.status": () => ({ sessions_running: 0, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => [{ id: "alpha", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true }],
    "session.list": () => listed,
    "workspace.list": () => workspaces,
    "section.list": () => [],
    "backend.list": () => [],
    "settings.get": () => (read ? read() : settings),
    "settings.set": (params: { patch: unknown }) => {
      const apply = () => {
        if (refuse) throw { kind: "rpc", error: { code: -32602, message: "settings.set: not a choice" } };
        settings = mergePatch(settings, params.patch);
        return {};
      };
      if (!hold) return apply();
      return new Promise((resolve, reject) => {
        gates.push({ open: () => { try { resolve(apply()); } catch (e) { reject(e); } } });
      });
    },
    "session.subscribe": () => ({ view: "v1", last_seq: 0 }),
    "session.unsubscribe": () => ({}),
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
  const writes = () => calls.filter((c) => c.method === "settings.set").map((c) => (c.params as { patch: unknown }).patch);
  return { client, writes, settings: () => settings, gates };
}

const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));
const rail = () => screen.getByRole("navigation", { name: "Rail" });

async function start(core: ReturnType<typeof fakeCore>) {
  render(createElement(App, { client: core.client }));
  await settle();
  await settle();
}

function arrangeItems() {
  fireEvent.click(within(rail()).getByRole("button", { name: "Arrange sessions" }));
  return [...screen.getAllByRole("menu")[0]!.querySelectorAll("li")];
}

async function openSettings() {
  arrangeItems();
  fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
  await settle();
}

const settingsPage = () => document.querySelector<HTMLElement>(".settings-page");
const chip = () => within(settingsPage()!).getByRole("button", { name: /^Archive inactive sessions:/ });

async function choose(label: string) {
  fireEvent.click(chip());
  fireEvent.click(screen.getByRole("option", { name: label }));
  await settle();
}

describe("the settings page", () => {
  it("opens from the arrange menu's last item, Settings, after Developer pane", async () => {
    await start(fakeCore());
    const items = arrangeItems();
    expect(items.at(-1)?.textContent).toBe("Settings");
    expect(items.at(-2)?.textContent).toBe("Developer pane");
    fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
    await settle();
    const page = settingsPage()!;
    expect(within(page).getByRole("heading", { level: 1, name: "Settings" })).toBeTruthy();
    // Storage is its first section and the one it opens on.
    const sections = within(page).getByRole("navigation", { name: "Settings sections" });
    const names = within(sections).getAllByRole("button");
    expect(names.map((n) => n.textContent)).toEqual(["Storage"]);
    expect(names[0]?.getAttribute("aria-current")).toBe("true");
    expect(within(page).getByRole("heading", { level: 2, name: "Storage" })).toBeTruthy();
    // No Save: a setting takes effect when it is changed.
    expect(within(page).queryByRole("button", { name: /save/i })).toBeNull();
  });

  it("hides what the sessions page hides, and keeps the active session active behind it", async () => {
    await start(fakeCore());
    expect(screen.getByRole("textbox", { name: /message/i })).toBeTruthy();
    await openSettings();
    expect(screen.queryByRole("textbox", { name: /message/i })).toBeNull();
    expect(screen.queryByRole("heading", { name: "Tidy the docs" })).toBeNull();
    expect(document.querySelector("main:not([hidden]) .session-controls")).toBeNull();
    expect(document.querySelector("main[hidden]")?.hasAttribute("inert")).toBe(true);
    expect(rail().querySelector('.session[aria-current="true"]')?.textContent).toContain("Tidy the docs");
    // Left by choosing the same tile, the session is as it was.
    fireEvent.click(within(rail()).getByText("Tidy the docs"));
    await settle();
    expect(settingsPage()).toBeNull();
    expect(screen.getByRole("textbox", { name: /message/i })).toBeTruthy();
  });

  it("leaves for New session and for View all", async () => {
    await start(fakeCore());
    await openSettings();
    fireEvent.click(within(rail()).getAllByRole("button", { name: "New session" })[0]!);
    await settle();
    expect(settingsPage()).toBeNull();
    await openSettings();
    arrangeItems();
    fireEvent.click(screen.getByRole("menuitem", { name: "View all" }));
    await settle();
    expect(settingsPage()).toBeNull();
    expect(document.querySelector(".sessions-page")).not.toBeNull();
  });
});

describe("Archive inactive sessions", () => {
  // verifies: FLOW-CLEAN-UP-R15
  it("offers Never, 1, 2, 7, 14 or 30 days, showing the value in force", async () => {
    await start(fakeCore({ sessions: { archive_inactive_days: 7 } }));
    await openSettings();
    expect(chip().getAttribute("aria-label")).toBe("Archive inactive sessions: 7 days");
    fireEvent.click(chip());
    const options = screen.getAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual(["Never", "1 day", "2 days", "7 days", "14 days", "30 days"]);
    expect(options.find((o) => o.getAttribute("aria-selected") === "true")?.textContent).toBe("7 days");
  });

  // verifies: FLOW-CLEAN-UP-R15
  it("shows Never while the setting is unset", async () => {
    await start(fakeCore({}));
    await openSettings();
    expect(chip().getAttribute("aria-label")).toBe("Archive inactive sessions: Never");
  });

  // verifies: FLOW-CLEAN-UP-R15
  it("writes a choice to `sessions.archive_inactive_days`, and shows it when the page opens again", async () => {
    const core = fakeCore({});
    await start(core);
    await openSettings();
    await choose("14 days");
    expect(core.writes()).toEqual([{ sessions: { archive_inactive_days: 14 } }]);
    expect(core.settings()).toEqual({ sessions: { archive_inactive_days: 14 } });
    expect(chip().getAttribute("aria-label")).toBe("Archive inactive sessions: 14 days");
    fireEvent.click(within(rail()).getByText("Tidy the docs"));
    await settle();
    await openSettings();
    expect(chip().getAttribute("aria-label")).toBe("Archive inactive sessions: 14 days");
  });

  // verifies: FLOW-CLEAN-UP-R15
  it("clears the setting when Never is chosen", async () => {
    const core = fakeCore({ sessions: { archive_inactive_days: 2 } });
    await start(core);
    await openSettings();
    await choose("Never");
    expect(core.writes()).toEqual([{ sessions: { archive_inactive_days: null } }]);
    expect(core.settings()).toEqual({ sessions: {} });
    expect(chip().getAttribute("aria-label")).toBe("Archive inactive sessions: Never");
  });

  it("shows a choice at once, busy and closed while Core answers", async () => {
    const core = fakeCore({}, { hold: true });
    await start(core);
    await openSettings();
    await choose("30 days");
    expect(chip().getAttribute("aria-label")).toBe("Archive inactive sessions: 30 days");
    expect(chip().getAttribute("aria-busy")).toBe("true");
    fireEvent.click(chip());
    expect(screen.queryByRole("listbox")).toBeNull();
    await act(async () => core.gates[0]!.open());
    await settle();
    expect(chip().hasAttribute("aria-busy")).toBe(false);
    expect(chip().getAttribute("aria-label")).toBe("Archive inactive sessions: 30 days");
  });

  it("goes back to the value in force and says why when Core refuses a change", async () => {
    await start(fakeCore({ sessions: { archive_inactive_days: 7 } }, { refuse: true }));
    await openSettings();
    await choose("1 day");
    expect(chip().getAttribute("aria-label")).toBe("Archive inactive sessions: 7 days");
    const problem = within(settingsPage()!).getByRole("alert");
    expect(problem.textContent).toContain("This setting wasn't changed.");
    expect(problem.textContent).toContain("not a choice");
  });

  // Until the settings are read the arrange button waits, so the page can't open on a setting still loading: its
  // look while loading is the component's and the gallery's.
  it("says why when the settings can't be read, and offers no choice", async () => {
    await start(fakeCore({}, { read: () => Promise.reject({ kind: "rpc", error: { code: -32603, message: "settings.get: the settings file is damaged" } }) }));
    await openSettings();
    expect(within(settingsPage()!).queryByRole("button", { name: /^Archive inactive sessions:/ })).toBeNull();
    const problem = within(settingsPage()!).getByRole("alert");
    expect(problem.textContent).toContain("This setting can't be read.");
    expect(problem.textContent).toContain("the settings file is damaged");
  });
});
