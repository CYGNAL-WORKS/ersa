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
function fakeCore(more: Answers = {}, start: { sessions?: Session[]; sections?: Section[] } = {}) {
  let sessions = start.sessions ?? listed;
  let sections = start.sections ?? madeSections;
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
    // As Core does (PROTO-CORE-API-R60, R76): spacing settled, a name refused blank, over 200 characters or holding a
    // control character other than spacing, and one exactly another's answered with that section, case and all.
    "section.create": (params: { name: string }) => {
      if ([...params.name].some((c) => /\p{Cc}/u.test(c) && !/\s/u.test(c)))
        throw { kind: "rpc", error: { code: -32602, message: "section.create: a section's name is one line, without control characters" } };
      const name = params.name.trim().replace(/\s+/g, " ");
      if (name === "") throw { kind: "rpc", error: { code: -32602, message: "section.create: a section's name cannot be blank" } };
      if ([...name].length > 200) throw { kind: "rpc", error: { code: -32602, message: "section.create: a section's name can have at most 200 characters" } };
      const taken = sections.find((s) => s.name === name);
      if (taken) return taken;
      const made = { id: `made-${sections.length}`, name, created_at: "2026-11-18T09:00:00Z" };
      sections = [...sections, made];
      return made;
    },
    // Core keeps each session's pin and section, and answers with the session as session.list lists it.
    "session.arrange": (params: { id: string; pinned?: boolean; section_id?: string | null }) => {
      sessions = sessions.map((s) => {
        if (s.id !== params.id) return s;
        const { section_id, ...rest } = s;
        const next = params.section_id === undefined ? section_id : (params.section_id ?? undefined);
        return { ...rest, pinned: params.pinned ?? s.pinned, ...(next ? { section_id: next } : {}) };
      });
      return sessions.find((s) => s.id === params.id);
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

async function start(more: Answers = {}, lists: Parameters<typeof fakeCore>[1] = {}) {
  const core = fakeCore(more, lists);
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

  // verifies: FLOW-ORGANIZE-SESSIONS-R34
  it("says why when the sections can't be read, and shows the sessions as grouped by none", async () => {
    await start({ "section.list": () => Promise.reject({ kind: "unavailable", message: "the database is locked" }) });
    expect(rail().textContent).toContain("Ersa can't read your sections.");
    expect(rail().textContent).toContain("the database is locked");
    // Every session is still in the rail, newest first under no heading, as grouping by none shows them.
    expect(groups()).toEqual([]);
    expect([...rail().querySelectorAll(".session-title")].map((el) => el.textContent)).toEqual(["Tidy the docs", "Buy stamps", "Write the notes", "Fix the build"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R35
  it("says it is loading while the sections are first read, and groups by them once they are", async () => {
    let answer: (sections: Section[]) => void = () => {};
    await start({ "section.list": () => new Promise<Section[]>((resolve) => (answer = resolve)) });
    expect(rail().textContent).toContain("Loading…");
    // No session is drawn one way and then regrouped.
    expect(rail().querySelectorAll(".session-title")).toHaveLength(0);
    await act(async () => answer(madeSections));
    expect(rail().textContent).not.toContain("Loading…");
    expect(groups().map(([name]) => name)).toEqual(["Work", "Errands", "Later", "No section"]);
  });
});

describe("a section heading's Delete", () => {
  // verifies: FLOW-ORGANIZE-SESSIONS-R05
  // verifies: FLOW-ORGANIZE-SESSIONS-R33
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

  // verifies: FLOW-ORGANIZE-SESSIONS-R33
  it("passes focus to the heading before the deleted section when none followed it", async () => {
    // Every session is in a section, so no "No section" heading follows the last one.
    await start({}, { sessions: listed.filter((s) => s.section_id !== undefined) });
    await chooseFromMore("Later", "Delete");
    expect(groups().map(([name]) => name)).toEqual(["Work", "Errands"]);
    expect(document.activeElement).toBe(heading("Errands")?.querySelector(".rail-heading-toggle"));
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R33
  it("passes focus to New session when no heading was beside the deleted section, a No section it makes included", async () => {
    // The one section holds every session: deleting it makes "No section", which did not follow it when Delete was chosen.
    await start({}, { sections: [madeSections[0]!], sessions: listed.slice(0, 2) });
    await chooseFromMore("Work", "Delete");
    expect(groups()).toEqual([["No section", ["Write the notes", "Fix the build"]]]);
    expect(document.activeElement).toBe(within(rail()).getByRole("button", { name: "New session" }));
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

// A tile, its More menu and Move to section ▸ (LAY-SHELL, A tile's menu).
const tileOf = (title: string) => [...rail().querySelectorAll<HTMLElement>(".session")].find((el) => el.querySelector(".session-title")?.textContent === title)!;
const pinnedTitles = () => [...rail().querySelectorAll(".rail-pinned .session-title")].map((el) => el.textContent);
const arranges = (core: ReturnType<typeof fakeCore>) => core.calls.filter((c) => c.method === "session.arrange").map((c) => c.params);
const creates = (core: ReturnType<typeof fakeCore>) => core.calls.filter((c) => c.method === "section.create").map((c) => c.params);
const nameField = () => screen.queryByRole("textbox", { name: "Section name" }) as HTMLInputElement | null;
// The headings after the Pinned group, a new section's field read as "field".
const headings = () => [...rail().querySelectorAll(".rail-projects > .rail-project")].map((g) => g.querySelector(".rail-heading-name")?.textContent ?? "field");

function openMoveTo(title: string) {
  const more = within(tileOf(title).closest("li")!).getByRole("button", { name: `More for ${title}` });
  more.focus();
  fireEvent.click(more, { detail: 0 });
  fireEvent.click(screen.getByRole("menuitem", { name: "Move to section" }));
  return within(screen.getByRole("menu", { name: "Move to section" }));
}

async function moveTo(title: string, item: string) {
  const submenu = openMoveTo(title);
  fireEvent.click(submenu.getByRole(item === "New section…" ? "menuitem" : "menuitemradio", { name: item }));
  await settle();
  await settle();
}

async function typeName(name: string, key = "Enter") {
  fireEvent.change(nameField()!, { target: { value: name } });
  fireEvent.keyDown(nameField()!, { key });
  await settle();
  await settle();
}

describe("a tile's Move to section", () => {
  // verifies: FLOW-ORGANIZE-SESSIONS-R03
  it("offers each section, the current one marked, then No section and New section…", async () => {
    await start();
    const submenu = openMoveTo("Buy stamps");
    const options = submenu.getAllByRole("menuitemradio").map((el) => [el.textContent, el.getAttribute("aria-checked")]);
    expect(options).toEqual([
      ["Work", "false"],
      ["Errands", "true"],
      ["Later", "false"],
      ["No section", "false"],
    ]);
    // New section… is an action after them, not an option.
    expect(submenu.getAllByRole("menuitem").map((el) => el.textContent)).toEqual(["New section…"]);
    fireEvent.keyDown(screen.getByRole("menu", { name: "Move to section" }), { key: "Escape" });
    fireEvent.keyDown(screen.getByRole("menu", { name: "More for Buy stamps" }), { key: "Escape" });
    // A closed menu fades out before it goes.
    await settle();
    // A session in no section has No section marked.
    const other = openMoveTo("Tidy the docs");
    expect(other.getByRole("menuitemradio", { name: "No section" }).getAttribute("aria-checked")).toBe("true");
    expect(other.getByRole("menuitemradio", { name: "Errands" }).getAttribute("aria-checked")).toBe("false");
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R03
  it("stores the move in Core, and the tile shows in its new section; No section takes it out of every one", async () => {
    const core = await start();
    await moveTo("Tidy the docs", "Errands");
    expect(arranges(core)).toEqual([{ id: "s4", section_id: "errands" }]);
    expect(groups()).toEqual([
      ["Work", ["Write the notes", "Fix the build"]],
      ["Errands", ["Tidy the docs", "Buy stamps"]],
      ["Later", []],
    ]);
    // Focus follows the tile to its new place (docs/design/RAIL.md, Menus).
    expect(document.activeElement).toBe(tileOf("Tidy the docs"));
    await moveTo("Fix the build", "No section");
    expect(arranges(core)[1]).toEqual({ id: "s1", section_id: null });
    expect(groups()[3]).toEqual(["No section", ["Fix the build"]]);
    // Choosing where it already is asks Core nothing.
    await moveTo("Buy stamps", "Errands");
    expect(arranges(core)).toHaveLength(2);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R22
  it("unpins a pinned session moved to a section in the one change, and keeps the pin moving it to No section", async () => {
    const core = await start({}, { sessions: [{ ...listed[3]!, pinned: true }, { ...listed[0]!, pinned: true }] });
    expect(pinnedTitles()).toEqual(["Tidy the docs", "Fix the build"]);
    await moveTo("Tidy the docs", "Later");
    expect(arranges(core)).toEqual([{ id: "s4", section_id: "later", pinned: false }]);
    expect(pinnedTitles()).toEqual(["Fix the build"]);
    expect(groups()[2]).toEqual(["Later", ["Tidy the docs"]]);
    await moveTo("Fix the build", "No section");
    expect(arranges(core)[1]).toEqual({ id: "s1", section_id: null });
    expect(pinnedTitles()).toEqual(["Fix the build"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R15
  it("leaves the tile where Core has it and says why when Core refuses the move", async () => {
    await start({
      "session.arrange": () => {
        throw { kind: "unavailable", message: "the database is locked" };
      },
    });
    await moveTo("Tidy the docs", "Errands");
    expect(groups()[1]).toEqual(["Errands", ["Buy stamps"]]);
    expect(groups()[3]).toEqual(["No section", ["Tidy the docs"]]);
    expect(rail().textContent).toContain("Ersa can't move this session.");
    expect(rail().textContent).toContain("the database is locked");
  });
});

describe("New section", () => {
  async function fromArrangeMenu() {
    fireEvent.click(within(rail()).getByRole("button", { name: "Arrange sessions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "New section" }));
    await settle();
  }

  it("shows a new heading with an empty name field after the sections, and Enter makes an empty section", async () => {
    const core = await start();
    await fromArrangeMenu();
    expect(document.activeElement).toBe(nameField());
    expect(nameField()?.value).toBe("");
    // Where Core will put it: after every section, before No section.
    expect(headings()).toEqual(["Work", "Errands", "Later", "field", "No section"]);
    await typeName("Someday");
    expect(creates(core)).toEqual([{ name: "Someday" }]);
    expect(nameField()).toBeNull();
    expect(headings()).toEqual(["Work", "Errands", "Later", "Someday", "No section"]);
    expect(arranges(core)).toEqual([]);
    // The field became the heading, which takes focus.
    expect(document.activeElement).toBe(heading("Someday")?.querySelector(".rail-heading-toggle"));
  });

  it("makes nothing on Escape or on Enter with a blank name, and focus goes back to what opened it", async () => {
    const core = await start();
    await fromArrangeMenu();
    await typeName("Someday", "Escape");
    expect(nameField()).toBeNull();
    expect(document.activeElement).toBe(within(rail()).getByRole("button", { name: "Arrange sessions" }));
    await moveTo("Tidy the docs", "New section…");
    await typeName("   ");
    expect(nameField()).toBeNull();
    // Back to the More button that opened the tile's menu (docs/design/RAIL.md, Menus).
    expect(document.activeElement).toBe(within(tileOf("Tidy the docs").closest("li")!).getByRole("button", { name: "More for Tidy the docs" }));
    expect(creates(core)).toEqual([]);
    expect(arranges(core)).toEqual([]);
    expect(headings()).toEqual(["Work", "Errands", "Later", "No section"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R22
  it("from a tile, makes the section and moves the session into it in one change, unpinning it", async () => {
    const core = await start({}, { sessions: [{ ...listed[3]!, pinned: true }] });
    await moveTo("Tidy the docs", "New section…");
    expect(document.activeElement).toBe(nameField());
    await typeName("Docs");
    expect(creates(core)).toEqual([{ name: "Docs" }]);
    expect(arranges(core)).toEqual([{ id: "s4", section_id: "made-3", pinned: false }]);
    expect(pinnedTitles()).toEqual([]);
    expect(groups()).toEqual([
      ["Work", []],
      ["Errands", []],
      ["Later", []],
      ["Docs", ["Tidy the docs"]],
    ]);
    // Focus follows the session's tile into the new section.
    expect(document.activeElement).toBe(tileOf("Tidy the docs"));
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R23
  it("gives the section already named exactly so, and makes a new one for a name that differs only in case", async () => {
    const core = await start();
    await moveTo("Tidy the docs", "New section…");
    await typeName("Errands");
    // Nothing new appears, and the session goes to the existing section.
    expect(headings()).toEqual(["Work", "Errands", "Later"]);
    expect(groups()[1]).toEqual(["Errands", ["Tidy the docs", "Buy stamps"]]);
    expect(arranges(core)).toEqual([{ id: "s4", section_id: "errands" }]);
    // "errands" is another section: the window leaves comparing names to Core and takes its answer.
    await fromArrangeMenu();
    await typeName("errands");
    expect(creates(core)).toEqual([{ name: "Errands" }, { name: "errands" }]);
    expect(headings()).toEqual(["Work", "Errands", "Later", "errands"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R23
  it("keeps the field and the name typed with Core's reason for a name over 200 characters, and moves nothing", async () => {
    const core = await start();
    await moveTo("Tidy the docs", "New section…");
    const long = "a".repeat(201);
    await typeName(long);
    expect(nameField()?.value).toBe(long);
    expect(screen.getByRole("alert").textContent).toBe("a section's name can have at most 200 characters");
    expect(document.activeElement).toBe(nameField());
    expect(arranges(core)).toEqual([]);
    expect(headings()).toEqual(["Work", "Errands", "Later", "field", "No section"]);
    // Typing again leaves the reason behind; Escape then makes nothing.
    fireEvent.change(nameField()!, { target: { value: "a".repeat(200) } });
    expect(screen.queryByRole("alert")).toBeNull();
    fireEvent.keyDown(nameField()!, { key: "Escape" });
    expect(nameField()).toBeNull();
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R27
  it("keeps the field with Core's reason for a name holding a control character, making nothing", async () => {
    const core = await start();
    await fromArrangeMenu();
    await typeName("Bell\u0007");
    expect(creates(core)).toEqual([{ name: "Bell\u0007" }]);
    expect(nameField()?.value).toBe("Bell\u0007");
    expect(screen.getByRole("alert").textContent).toBe("a section's name is one line, without control characters");
    expect(headings()).toEqual(["Work", "Errands", "Later", "field", "No section"]);
    // Spacing is not refused: a tab is one space, as Core takes it.
    await typeName("Odd\tjobs");
    expect(headings()).toEqual(["Work", "Errands", "Later", "Odd jobs", "No section"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R36
  it("switches the grouping to Section when chosen under another grouping, from either menu", async () => {
    const core = await start({ "settings.get": () => ({ rail: { group_by: "date" } }) });
    const groupings = () => core.calls.filter((c) => c.method === "settings.set").map((c) => c.params);
    await fromArrangeMenu();
    expect(groupings()).toEqual([{ patch: { rail: { group_by: "section" } } }]);
    expect(headings()).toEqual(["Work", "Errands", "Later", "field", "No section"]);
    fireEvent.keyDown(nameField()!, { key: "Escape" });
    // Grouped by project again, then New section… from a tile.
    fireEvent.click(within(rail()).getByRole("button", { name: "Arrange sessions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /^Group by/ }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Project" }));
    await settle();
    expect(headings()).toEqual(["alpha"]);
    await moveTo("Tidy the docs", "New section…");
    expect(groupings().at(-1)).toEqual({ patch: { rail: { group_by: "section" } } });
    expect(headings()).toEqual(["Work", "Errands", "Later", "field", "No section"]);
  });

  it("is unavailable, saying why, while the sections can't be read", async () => {
    await start({ "section.list": () => Promise.reject({ kind: "unavailable", message: "the database is locked" }) });
    fireEvent.click(within(rail()).getByRole("button", { name: "Arrange sessions" }));
    const item = screen.getByRole("menuitem", { name: /^New section/ });
    expect(item.getAttribute("aria-disabled")).toBe("true");
    expect(item.textContent).toContain("Ersa can't read your sections.");
  });
});

describe("New section from the sessions page", () => {
  it("gives focus back to the row's More when nothing is made", async () => {
    const core = await start();
    fireEvent.click(within(rail()).getByRole("button", { name: "Arrange sessions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "View all" }));
    await settle();
    const row = [...document.querySelectorAll<HTMLElement>(".sessions-page-row")].find((el) => el.textContent?.includes("Tidy the docs"))!;
    const more = within(row).getByRole("button", { name: "More for Tidy the docs" });
    more.focus();
    fireEvent.click(more, { detail: 0 });
    fireEvent.click(screen.getByRole("menuitem", { name: "Move to section" }));
    fireEvent.click(within(screen.getByRole("menu", { name: "Move to section" })).getByRole("menuitem", { name: "New section…" }));
    await settle();
    // The heading shows in the rail beside the page, and takes focus.
    expect(document.activeElement).toBe(nameField());
    await typeName("Docs", "Escape");
    expect(nameField()).toBeNull();
    expect(document.activeElement).toBe(more);
    expect(creates(core)).toEqual([]);
  });
});

describe("a new section being made for a session", () => {
  // verifies: FLOW-ORGANIZE-SESSIONS-R22
  it("holds Pin and Move to section until Core has made it, so the move unpins the session as it stands", async () => {
    let made: (section: Section) => void = () => {};
    const core = await start({ "section.create": () => new Promise<Section>((resolve) => (made = resolve)) });
    await moveTo("Tidy the docs", "New section…");
    await typeName("Docs");
    // While Core makes it, the session's Pin waits, as for any change on its way.
    fireEvent.click(within(tileOf("Tidy the docs").closest("li")!).getByRole("button", { name: "More for Tidy the docs" }));
    const pin = screen.getByRole("menuitem", { name: /^Pin/ });
    expect(pin.getAttribute("aria-disabled")).toBe("true");
    expect(pin.textContent).toContain("Saving…");
    fireEvent.click(pin);
    fireEvent.keyDown(screen.getByRole("menu", { name: "More for Tidy the docs" }), { key: "Escape" });
    await act(async () => made({ id: "docs", name: "Docs", created_at: "2026-11-18T09:00:00Z" }));
    await settle();
    expect(arranges(core)).toEqual([{ id: "s4", section_id: "docs" }]);
  });
});
