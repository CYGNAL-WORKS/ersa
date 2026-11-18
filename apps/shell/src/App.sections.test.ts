// @vitest-environment jsdom
// Grouping the rail by section and a section heading's More menu in the window, against a fake Core
// (FLOW-ORGANIZE-SESSIONS, steps 4 and 5): the headings drawn from Core's sections, deleting a section, renaming one.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, Method } from "./core/client";
import type { CoreHelloResult, Section, Session, Workspace } from "./protocol/core-api.gen";

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
  unseen: false,
  tile: { state: "idle", pending: [] },
  ...more,
});

// Core lists sections in the order they were made: "Work" first though "Errands" sorts before it by name, and
// "Later" holds nothing.
const madeSections: Section[] = [
  { id: "work", name: "Work", created_at: "2026-11-01T09:00:00Z" },
  { id: "errands", name: "Errands", created_at: "2026-11-02T09:00:00Z" },
  { id: "later", name: "Later", created_at: "2026-11-03T09:00:00Z" },
];
const listed = [
  session("s1", "Fix the build", "2026-11-01T09:00:00Z", { section_id: "work" }),
  session("s2", "Write the notes", "2026-11-02T09:00:00Z", { section_id: "work" }),
  session("s3", "Buy stamps", "2026-11-03T09:00:00Z", { section_id: "errands" }),
  session("s4", "Tidy the docs", "2026-11-04T09:00:00Z"),
];

type Answers = Partial<Record<Method, (params: never) => unknown>>;

// Core keeps the sections and the sessions' membership: section.remove and section.rename change what the lists
// answer, as Core's do.
function fakeCore(more: Answers = {}) {
  let sessions = listed;
  let sections = madeSections;
  const calls: { method: Method; params: unknown }[] = [];
  const answers: Answers = {
    "core.status": () => ({ sessions_running: 0, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => [{ id: "alpha", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true }],
    "session.list": () => sessions,
    "section.list": () => sections,
    "workspace.list": () => workspaces,
    "backend.list": () => [],
    "settings.get": () => ({ rail: { group_by: "section" } }),
    "settings.set": () => ({}),
    "session.subscribe": () => ({ view: "v1", last_seq: 0 }),
    "session.unsubscribe": () => ({}),
    "section.remove": (params: { id: string }) => {
      const gone = sections.find((s) => s.id === params.id);
      sections = sections.filter((s) => s.id !== params.id);
      sessions = sessions.map(({ section_id, ...rest }) => (section_id === params.id ? rest : { ...rest, ...(section_id ? { section_id } : {}) }));
      return gone;
    },
    "section.rename": (params: { id: string; name: string }) => {
      // Core settles the spacing, so the name it answers with is the one shown.
      const renamed = { ...sections.find((s) => s.id === params.id)!, name: params.name.replace(/\s+/g, " ") };
      sections = sections.map((s) => (s.id === params.id ? renamed : s));
      return renamed;
    },
    ...more,
  };
  const client: CoreClient = {
    attach: () => Promise.resolve<Attachment>({ how: "attached", hello }),
    call: (method, params) => {
      calls.push({ method, params });
      const answer = answers[method];
      if (!answer) return Promise.reject({ kind: "unavailable", message: `no answer for ${method}` }) as never;
      return new Promise((resolve, reject) => {
        try {
          resolve(answer(params as never));
        } catch (error) {
          reject(error);
        }
      }) as never;
    },
  };
  return { client, calls };
}

const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));
const rail = () => screen.getByRole("navigation", { name: "Rail" });
// Each heading drawn after the Pinned group, and the tiles under it.
const groups = () =>
  [...rail().querySelectorAll<HTMLElement>(".rail-projects > .rail-project")].map((group) => [
    group.querySelector(".rail-heading-name")?.textContent,
    [...group.querySelectorAll(".session-title")].map((el) => el.textContent),
  ]);
const heading = (name: string) => [...rail().querySelectorAll<HTMLElement>(".rail-heading")].find((h) => h.querySelector(".rail-heading-name")?.textContent === name);

async function start(more: Answers = {}) {
  const core = fakeCore(more);
  render(createElement(App, { client: core.client }));
  await settle();
  await settle();
  return core;
}

async function chooseFromMore(section: string, item: string) {
  const more = within(heading(section)!).getByRole("button", { name: `More for ${section}` });
  more.focus();
  // A click the keyboard made, as Enter on the focused button gives.
  fireEvent.click(more, { detail: 0 });
  fireEvent.click(screen.getByRole("menuitem", { name: item }));
  await settle();
  await settle();
}

describe("the rail grouped by section", () => {
  // verifies: FLOW-ORGANIZE-SESSIONS-R04
  it("shows every section Core has, empty or not, in the order they were made, then No section", async () => {
    await start();
    expect(groups()).toEqual([
      ["Work", ["Write the notes", "Fix the build"]],
      ["Errands", ["Buy stamps"]],
      ["Later", []],
      ["No section", ["Tidy the docs"]],
    ]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R04
  it("groups by section when the user chooses it from the arrange menu, and Core keeps the choice", async () => {
    const core = await start({ "settings.get": () => ({}) });
    expect(groups().map(([name]) => name)).toEqual(["alpha"]);
    fireEvent.click(within(rail()).getByRole("button", { name: "Arrange sessions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /^Group by/ }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Section" }));
    await settle();
    expect(core.calls.filter((c) => c.method === "settings.set").map((c) => c.params)).toEqual([{ patch: { rail: { group_by: "section" } } }]);
    expect(groups().map(([name]) => name)).toEqual(["Work", "Errands", "Later", "No section"]);
  });

  it("waits for the sections before grouping by them, and says why when they can't be read", async () => {
    await start({ "section.list": () => Promise.reject({ kind: "unavailable", message: "the database is locked" }) });
    expect(rail().textContent).toContain("Ersa can't read your sections.");
    expect(rail().textContent).toContain("the database is locked");
    // Every session is still in the rail, under no heading.
    expect([...rail().querySelectorAll(".session-title")].map((el) => el.textContent)).toEqual(["Tidy the docs", "Buy stamps", "Write the notes", "Fix the build"]);
  });
});

describe("a section heading's Delete", () => {
  // verifies: FLOW-ORGANIZE-SESSIONS-R05
  it("deletes the section asking nothing, and puts its sessions in No section, keeping every session", async () => {
    const core = await start();
    const listings = () => core.calls.filter((c) => c.method === "session.list").length;
    const before = listings();
    await chooseFromMore("Work", "Delete");
    // Core moved the sessions, so the window reads them again rather than keep their old section.
    expect(listings()).toBeGreaterThan(before);
    expect(core.calls.filter((c) => c.method.startsWith("section.") && c.method !== "section.list").map((c) => [c.method, c.params])).toEqual([
      ["section.remove", { id: "work" }],
    ]);
    // Nothing asked first, and no session archived, deleted or moved by the window.
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(core.calls.map((c) => c.method)).not.toContain("session.arrange");
    expect(core.calls.map((c) => c.method)).not.toContain("session.delete");
    expect(groups()).toEqual([
      ["Errands", ["Buy stamps"]],
      ["Later", []],
      ["No section", ["Tidy the docs", "Write the notes", "Fix the build"]],
    ]);
    // Focus, given back to the gone heading's More, moves to the heading after it.
    expect(document.activeElement).toBe(heading("Errands")?.querySelector(".rail-heading-toggle"));
  });

  it("keeps the section and says why when Core refuses", async () => {
    await start({
      "section.remove": () => {
        throw { kind: "unavailable", message: "the database is locked" };
      },
    });
    await chooseFromMore("Work", "Delete");
    expect(groups()[0]).toEqual(["Work", ["Write the notes", "Fix the build"]]);
    expect(rail().textContent).toContain("Ersa can't delete this section.");
    expect(rail().textContent).toContain("the database is locked");
  });
});

describe("a section heading's Rename", () => {
  const field = () => screen.getByRole("textbox", { name: "Section name" }) as HTMLInputElement;

  it("turns the name into a field in place, and Enter saves the name Core keeps", async () => {
    const core = await start();
    await chooseFromMore("Errands", "Rename");
    expect(document.activeElement).toBe(field());
    expect(field().value).toBe("Errands");
    fireEvent.change(field(), { target: { value: "Odd   jobs" } });
    fireEvent.keyDown(field(), { key: "Enter" });
    await settle();
    expect(core.calls.filter((c) => c.method === "section.rename").map((c) => c.params)).toEqual([{ id: "errands", name: "Odd   jobs" }]);
    expect(screen.queryByRole("textbox", { name: "Section name" })).toBeNull();
    expect(groups().map(([name]) => name)).toEqual(["Work", "Odd jobs", "Later", "No section"]);
    // Focus goes back to the heading.
    expect(document.activeElement).toBe(heading("Odd jobs")?.querySelector(".rail-heading-toggle"));
  });

  it("keeps the field and the name typed with Core's reason when Core refuses it, and Escape restores the name", async () => {
    const core = await start({
      "section.rename": () => {
        throw { kind: "rpc", error: { code: -32602, message: "section.rename: another section is named Work" } };
      },
    });
    await chooseFromMore("Errands", "Rename");
    fireEvent.change(field(), { target: { value: "Work" } });
    fireEvent.keyDown(field(), { key: "Enter" });
    await settle();
    expect(field().value).toBe("Work");
    expect(screen.getByRole("alert").textContent).toBe("another section is named Work");
    fireEvent.keyDown(field(), { key: "Escape" });
    expect(screen.queryByRole("textbox", { name: "Section name" })).toBeNull();
    expect(groups().map(([name]) => name)).toEqual(["Work", "Errands", "Later", "No section"]);
    expect(core.calls.filter((c) => c.method === "section.rename")).toHaveLength(1);
  });
});
