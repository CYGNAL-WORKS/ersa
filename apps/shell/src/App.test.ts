// @vitest-environment jsdom
// The window's wiring, rendered in a DOM against a fake Core: what App hands the new-session screen.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { mergePatch } from "./session/arrangement";
import type { Attachment, CoreClient, CoreEvent, Method } from "./core/client";
import type { DragDrop, ShellControl } from "./core/shell";
import type { BackendInfo, CoreHelloResult, Project, Session, Workspace } from "./protocol/core-api.gen";

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const project = (id: string): Project => ({ id, host_id: "local", path: `D:/code/${id}`, name: id, vcs: "git", trusted: true });
const root = (project_id: string): Workspace => ({ id: `w-${project_id}`, project_id, kind: "root", path: `D:/code/${project_id}`, vcs_ref: "main", state: "ready" });
const session = (id: string, project_id: string, created_at: string): Session => ({
  id,
  workspace_id: `w-${project_id}`,
  host_id: "local",
  backend: "claude-code",
  model: "opus",
  effort: "medium",
  permission_mode: "default",
  state: "ended",
  created_at,
  archived: false,
  pinned: false,
  tile: { state: "ended", pending: [] },
});

// Core lists alpha first; beta has the newest session, so the screen lists beta first.
const projects = [project("alpha"), project("beta"), project("gamma")];
const sessions = [session("s1", "alpha", "2026-11-01T09:00:00Z"), session("s2", "beta", "2026-11-02T09:00:00Z")];

type Answers = Partial<Record<Method, (params: never) => unknown>>;

/**
 * A Core that answers the reads App makes at launch, attaches once (or each time, with `reattach`), and can drop
 * the connection. `more` adds or replaces answers; `calls` records every request, in order.
 */
function fakeCore(more: Answers = {}, { reattach = false } = {}) {
  let onEvent: (event: CoreEvent) => void = () => {};
  let attaches = 0;
  const calls: { method: Method; params: unknown }[] = [];
  let settings: unknown = {};
  const answers: Answers = {
    "core.status": () => ({ sessions_running: 0, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => projects,
    "session.list": () => sessions,
    "workspace.list": (params: { project_id: string }) => [root(params.project_id)],
    "backend.list": () => [],
    // Settings as Core keeps them: each patch merged in, and read back on the next connection.
    "settings.get": () => settings,
    "settings.set": (params: { patch: unknown }) => {
      settings = mergePatch(settings, params.patch);
      return {};
    },
    ...more,
  };
  const client: CoreClient = {
    attach: (listener) => {
      onEvent = listener;
      // Attaching again after a loss never answers: Core stays away.
      return attaches++ === 0 || reattach ? Promise.resolve<Attachment>({ how: "attached", hello }) : new Promise(() => {});
    },
    call: (method, params) => {
      calls.push({ method, params });
      const answer = answers[method];
      if (!answer) return Promise.reject({ kind: "unavailable", message: `no answer for ${method}` }) as never;
      // An answer that throws is Core refusing the request.
      return new Promise((resolve) => resolve(answer(params as never))) as never;
    },
  };
  return { client, calls, drop: () => act(() => onEvent({ kind: "lost", reason: "gone" })), emit: (event: CoreEvent) => act(() => onEvent(event)) };
}

/** Lets every answer the fake gave settle, and the renders they cause. */
const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));

/** The project chip's list, in order, opened if it is not. */
function projectList(): string[] {
  const chip = screen.getByRole("button", { name: /^Project:/ });
  if (chip.getAttribute("aria-expanded") !== "true") fireEvent.click(chip);
  const list = screen.getByRole("listbox", { name: "Project" });
  return within(list)
    .getAllByRole("option")
    .map((o) => o.getAttribute("data-id"))
    .filter((id): id is string => id !== null);
}

async function openScreen() {
  const core = fakeCore();
  render(createElement(App, { client: core.client }));
  await settle();
  await settle();
  // The newest session shows at launch; New session for gamma opens the screen.
  fireEvent.click(screen.getByRole("button", { name: "New session in gamma" }));
  return core;
}

afterEach(cleanup);

describe("the projects App gives the new-session screen", () => {
  // verifies: FLOW-NEW-SESSION-R14
  it("lists them most recently used first, by the sessions Core lists", async () => {
    await openScreen();
    expect(projectList()).toEqual(["beta", "alpha", "gamma"]);
  });

  // verifies: FLOW-NEW-SESSION-R14
  it("keeps that order while the session list cannot be read, rather than falling back to Core's", async () => {
    const core = await openScreen();
    expect(projectList()).toEqual(["beta", "alpha", "gamma"]);
    core.drop();
    await settle();
    expect(projectList()).toEqual(["beta", "alpha", "gamma"]);
  });
});

// Adding a project from the new-session screen (FLOW-NEW-SESSION-R17 to R20, R27): the sequence against a fake Core, with
// the Shell's folder dialog replaced by a stand-in that answers each pick in turn with a folder or a cancel (null).

const claude: BackendInfo = {
  name: "claude-code",
  label: "Claude Code",
  version: "2.1.302",
  status: "available",
  tested: { min: "2.1.0", max: "2.1.400" },
  models: [{ id: "opus", label: "Opus", default: true }],
  efforts: [{ id: "high", label: "High", default: true }],
  permission_modes: [{ id: "default", label: "Ask before acting", default: true }],
};

/** A Shell whose folder dialog answers each pick with the next of `picks`, then with a cancel. */
function fakeShell(...picks: (string | null)[]) {
  return {
    onQuitRequested: () => () => {},
    quitHeard: async () => {},
    show: async () => false,
    hide: async () => {},
    quit: async () => {},
    pickFolder: vi.fn(async () => picks.shift() ?? null),
    pickFiles: vi.fn(async (_images: boolean) => [] as string[]),
    onDragDrop: vi.fn((_handler: (event: DragDrop) => void) => () => {}),
  } satisfies ShellControl;
}

/**
 * The new-session screen with `listed` as Core's projects and no sessions, a backend to start with, and the
 * project.find and project.add answers a real Core gives: a folder added is found and listed after.
 */
async function screenWith(listed: Project[], shell?: ShellControl, more: Answers = {}) {
  const known = [...listed];
  const core = fakeCore({
    "project.list": () => known,
    "session.list": () => [],
    "backend.list": () => [claude],
    "project.find": (params: { path: string }) => known.find((p) => p.path === params.path) ?? null,
    "project.add": (params: { host_id: string; path: string; trusted: boolean }) => {
      const added: Project = { id: "p-new", host_id: params.host_id, path: params.path, name: "stranger", vcs: "git", trusted: params.trusted };
      known.push(added);
      return added;
    },
    ...more,
  });
  render(createElement(App, { client: core.client, shell }));
  await settle();
  await settle();
  return core;
}

const chipValue = () => screen.getByRole("button", { name: /^Project:/ }).getAttribute("aria-label");
const words = () => screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
const methods = (core: { calls: { method: Method }[] }) => core.calls.map((c) => c.method);
const paramsOf = (core: { calls: { method: Method; params: unknown }[] }, method: Method) => core.calls.find((c) => c.method === method)?.params;
const trustQuestion = () => screen.queryByRole("region", { name: /^Trust / });

/** Chooses Add project… from the project chip's list, and lets the dialog and Core answer. */
async function addFromChip() {
  const chip = screen.getByRole("button", { name: /^Project:/ });
  if (chip.getAttribute("aria-expanded") !== "true") fireEvent.click(chip);
  fireEvent.click(screen.getByRole("option", { name: /Add project…/ }));
  await settle();
}

describe("adding a project from the new-session screen", () => {
  // verifies: FLOW-NEW-SESSION-R27
  it("opens the folder dialog from Add project…, asks about trust for a new folder, then adds it trusted and chooses it", async () => {
    const shell = fakeShell("D:/clones/stranger");
    const core = await screenWith([project("alpha")], shell);
    await addFromChip();
    expect(shell.pickFolder).toHaveBeenCalledTimes(1);
    expect(paramsOf(core, "project.find")).toEqual({ host_id: "local", path: "D:/clones/stranger" });

    // The question is asked before anything is added.
    const question = screen.getByRole("region", { name: "Trust stranger?" });
    expect(methods(core)).not.toContain("project.add");

    fireEvent.click(within(question).getByRole("button", { name: "Trust" }));
    await settle();
    expect(paramsOf(core, "project.add")).toEqual({ host_id: "local", path: "D:/clones/stranger", trusted: true });
    expect(trustQuestion()).toBeNull();
    expect(chipValue()).toBe("Project: stranger");
    expect(document.activeElement).toBe(words());
  });

  // verifies: FLOW-NEW-SESSION-R24
  it("asks as the CLI does: the folder's path, the safety check, and what agents can do there, and nothing more", async () => {
    await screenWith([project("alpha")], fakeShell("D:/clones/stranger"));
    await addFromChip();
    const question = screen.getByRole("region", { name: "Trust stranger?" });
    const lines = [...question.querySelectorAll("p")].map((p) => p.textContent);
    expect(lines).toEqual([
      "Trust stranger?",
      "D:/clones/stranger",
      "Is this a project you created or one you trust, like your own code, a well-known open source project, or work from your team? If not, review what's in this folder first.",
      "Agents will be able to read, edit, and run files here.",
    ]);
    expect(question.textContent).not.toMatch(/repositor/i);
  });

  // verifies: FLOW-NEW-SESSION-R25
  it("offers Cancel, focused, then Trust, which adds the project trusted, and no way to add it untrusted", async () => {
    const core = await screenWith([project("alpha")], fakeShell("D:/clones/stranger", "D:/clones/stranger"));
    await addFromChip();
    let question = screen.getByRole("region", { name: "Trust stranger?" });
    const answers = within(question).getAllByRole("button");
    expect(answers.map((b) => b.textContent)).toEqual(["Cancel", "Trust"]);
    // Neither answer is primary: trusting is never the default.
    expect(answers.map((b) => b.getAttribute("data-variant"))).toEqual(["quiet", "quiet"]);
    expect(document.activeElement).toBe(answers[0]);

    // The arrows move between the answers; Cancel, as Enter on it does, adds nothing.
    fireEvent.keyDown(answers[0]!, { key: "ArrowRight" });
    expect(document.activeElement).toBe(answers[1]);
    fireEvent.keyDown(answers[1]!, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(answers[0]);
    // Away from the answers, as in the explanation when it scrolls, the arrows are left alone.
    fireEvent.keyDown(question.querySelector(".trust-question-body")!, { key: "ArrowDown" });
    expect(document.activeElement).toBe(answers[0]);
    fireEvent.click(answers[0]!);
    await settle();
    expect(trustQuestion()).toBeNull();
    expect(methods(core)).not.toContain("project.add");

    await addFromChip();
    question = screen.getByRole("region", { name: "Trust stranger?" });
    fireEvent.click(within(question).getByRole("button", { name: "Trust" }));
    await settle();
    expect(core.calls.filter((c) => c.method === "project.add").map((c) => c.params)).toEqual([
      { host_id: "local", path: "D:/clones/stranger", trusted: true },
    ]);
    expect(chipValue()).toBe("Project: stranger");
  });

  // verifies: FLOW-NEW-SESSION-R27
  it("opens the folder dialog on Ctrl+O, the shortcut shown beside Add project…", async () => {
    const shell = fakeShell("D:/clones/stranger");
    await screenWith([project("alpha")], shell);
    fireEvent.click(screen.getByRole("button", { name: /^Project:/ }));
    expect(screen.getByRole("option", { name: /Add project…/ }).textContent).toContain("Ctrl+O");
    fireEvent.keyDown(words(), { key: "o", ctrlKey: true });
    await settle();
    expect(shell.pickFolder).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("region", { name: "Trust stranger?" })).toBeTruthy();
  });

  // verifies: FLOW-NEW-SESSION-R27
  it("leaves Add project unavailable, without its shortcut, when there is no folder dialog to open", async () => {
    await screenWith([project("alpha")]);
    fireEvent.click(screen.getByRole("button", { name: /^Project:/ }));
    const add = screen.getByRole("option", { name: /Add project…/ });
    expect(add.getAttribute("aria-disabled")).toBe("true");
    expect(add.textContent).not.toContain("Ctrl+O");
  });

  // verifies: FLOW-NEW-SESSION-R17
  it("changes nothing when the folder dialog is canceled", async () => {
    const core = await screenWith([project("alpha")], fakeShell(null));
    fireEvent.change(words(), { target: { value: "Fix the build" } });
    await addFromChip();
    expect(methods(core)).not.toContain("project.find");
    expect(methods(core)).not.toContain("project.add");
    expect(trustQuestion()).toBeNull();
    expect(chipValue()).toBe("Project: alpha");
    expect(words().value).toBe("Fix the build");
    expect(document.activeElement).toBe(words());
  });

  // verifies: FLOW-NEW-SESSION-R17
  it.each([
    ["Cancel", () => fireEvent.click(screen.getByRole("button", { name: "Cancel" }))],
    ["Escape", () => fireEvent.keyDown(screen.getByRole("button", { name: "Cancel" }), { key: "Escape" })],
  ])("changes nothing when the trust question is closed with %s", async (_, close) => {
    const core = await screenWith([project("alpha")], fakeShell("D:/clones/stranger"));
    fireEvent.change(words(), { target: { value: "Fix the build" } });
    await addFromChip();
    close();
    await settle();
    expect(trustQuestion()).toBeNull();
    expect(methods(core)).not.toContain("project.add");
    expect(chipValue()).toBe("Project: alpha");
    expect(words().value).toBe("Fix the build");
    expect(document.activeElement).toBe(words());
  });

  // verifies: FLOW-NEW-SESSION-R18
  it("chooses a folder that is already a project without asking about trust", async () => {
    const core = await screenWith([project("alpha"), project("beta")], fakeShell("D:/code/beta"));
    expect(chipValue()).toBe("Project: alpha");
    await addFromChip();
    expect(paramsOf(core, "project.find")).toEqual({ host_id: "local", path: "D:/code/beta" });
    expect(trustQuestion()).toBeNull();
    expect(methods(core)).not.toContain("project.add");
    expect(chipValue()).toBe("Project: beta");
  });

  // verifies: FLOW-NEW-SESSION-R19
  it("with no project, offers to choose a folder, and sending opens the dialog and keeps the message", async () => {
    const shell = fakeShell("D:/clones/stranger");
    const core = await screenWith([], shell);
    expect(chipValue()).toBe("Project: Choose a folder");
    fireEvent.click(screen.getByRole("button", { name: /^Project:/ }));
    const options = within(screen.getByRole("listbox", { name: "Project" })).getAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual(["Add project…Ctrl+O"]);
    fireEvent.click(screen.getByRole("button", { name: /^Project:/ }));

    fireEvent.change(words(), { target: { value: "Fix the build" } });
    await act(async () => {
      fireEvent.keyDown(words(), { key: "Enter" });
    });
    await settle();
    expect(shell.pickFolder).toHaveBeenCalledTimes(1);
    expect(words().value).toBe("Fix the build");

    fireEvent.click(screen.getByRole("button", { name: "Trust" }));
    await settle();
    expect(chipValue()).toBe("Project: stranger");
    // The message waits for the user to send it: nothing was started.
    expect(words().value).toBe("Fix the build");
    expect(methods(core)).not.toContain("workspace.create");
    expect(methods(core)).not.toContain("session.create");
  });

  // verifies: FLOW-NEW-SESSION-R20
  it("offers no field for typing a folder path, on the screen or in the rail", async () => {
    await screenWith([], fakeShell());
    expect(screen.getAllByRole("textbox").map((t) => t.getAttribute("aria-label"))).toEqual(["Message"]);
  });

  it("says why when Core can't look the folder up, and adds nothing", async () => {
    const core = await screenWith([project("alpha")], fakeShell("D:/clones/stranger"), {
      "project.find": () => {
        throw { kind: "rpc", error: { code: -32602, message: "project.find: invalid params", data: { detail: "D:/clones/stranger is not a folder" } } };
      },
    });
    await addFromChip();
    expect(screen.getByRole("alert").textContent).toBe("D:/clones/stranger is not a folder");
    expect(trustQuestion()).toBeNull();
    expect(methods(core)).not.toContain("project.add");
    expect(chipValue()).toBe("Project: alpha");
  });
});

// The rail's wiring (LAY-SHELL, The rail; FLOW-NEW-SESSION-R13): App hands the tree the sessions and the workspaces it
// read, so each session sits under its own project. session/tree.test.ts covers the grouping itself.
describe("the tree App draws in the rail", () => {
  /** Each project heading's name with how many tiles sit beneath it, in the rail's order. */
  function headings(): [string, number][] {
    const tree = screen.getByRole("region", { name: "Projects and sessions" });
    // The Pinned group, empty, is drawn closed and hidden from everyone, so it is not one of the headings shown.
    return [...tree.querySelectorAll(".rail-project:not([data-empty] > *)")].map((group) => [
      group.querySelector(".rail-heading-name")?.textContent ?? "",
      group.querySelectorAll(".session").length,
    ]);
  }

  // verifies: FLOW-NEW-SESSION-R13
  it("puts each session Core lists under its own project, by the workspaces App read", async () => {
    render(createElement(App, { client: fakeCore().client }));
    await settle();
    await settle();
    expect(headings()).toEqual([
      ["alpha", 1],
      ["beta", 1],
      ["gamma", 0],
    ]);
    // Every session is placed, so none sits at the end under no heading.
    const tree = screen.getByRole("region", { name: "Projects and sessions" });
    expect(tree.querySelectorAll(":scope > .rail-tiles")).toHaveLength(0);
  });

  // A session made by another window: its workspace is read before its tile is drawn, so it never sits at the end
  // first. A tile already at the end stays where it is meanwhile.
  async function unknownAtFirst(lists: boolean) {
    const made = { ...root("alpha"), id: "w-new", kind: "isolated" as const };
    const stranded = { ...session("lost", "alpha", "2026-11-01T09:00:00Z"), workspace_id: "w-lost" };
    const fresh = { ...session("fresh", "alpha", "2026-11-08T09:00:00Z"), workspace_id: "w-new" };
    const later: (() => void)[] = [];
    let listed = [stranded];
    let holding = false;
    const core = fakeCore({
      "session.list": () => listed,
      "workspace.list": (params: { project_id: string }) => {
        if (!holding) return [root(params.project_id)];
        const answer = [root(params.project_id), ...(lists && params.project_id === "alpha" ? [made] : [])];
        return new Promise((resolve) => later.push(() => resolve(answer)));
      },
    });
    render(createElement(App, { client: core.client }));
    for (let turn = 0; turn < 20 && !document.querySelector('[data-session-id="lost"]'); turn++) await settle();
    await settle();
    const tile = document.querySelector('[data-session-id="lost"]');
    expect(tile).not.toBeNull();
    holding = true;
    listed = [stranded, fresh];
    core.emit({ kind: "notification", method: "session.tile", params: { session_id: "fresh", tile: { state: "idle", pending: [] } } } as CoreEvent);
    for (let turn = 0; turn < 20 && later.length < projects.length; turn++) await settle();
    const tree = screen.getByRole("region", { name: "Projects and sessions" });
    expect(later).toHaveLength(projects.length);
    expect(tree.querySelector('[data-session-id="fresh"]')).toBeNull();
    expect(tree.querySelector('[data-session-id="lost"]')).toBe(tile);
    await act(async () => later.forEach((answer) => answer()));
    await settle();
    await settle();
    return tree;
  }

  it("holds a new session's tile back until the read for its workspace answers, then draws it under its project", async () => {
    const tree = await unknownAtFirst(true);
    expect(headings()[0]).toEqual(["alpha", 1]);
    expect(tree.querySelector('.rail-project [data-session-id="fresh"]')).not.toBeNull();
  });

  // verifies: FLOW-NEW-SESSION-R13
  it("puts a session it starts under its project as soon as Core lists it, while the first message is still on its way", async () => {
    const made = { ...session("s-new", "alpha", "2026-11-03T09:00:00Z"), workspace_id: "w-new", state: "idle" as const };
    let created = false;
    const core = await screenWith([project("alpha")], undefined, {
      "workspace.create": (params: { project_id: string }) => ({ ...root(params.project_id), id: "w-new", kind: "isolated", vcs_ref: "ersa/new" }),
      "session.create": () => ((created = true), made),
      "session.list": () => (created ? [made] : []),
      "session.send": () => new Promise(() => {}),
    });
    fireEvent.change(words(), { target: { value: "Fix it" } });
    fireEvent.keyDown(words(), { key: "Enter" });
    await settle();
    await settle();
    core.emit({ kind: "notification", method: "session.tile", params: { session_id: "s-new", tile: { state: "idle", pending: [] } } } as CoreEvent);
    await settle();
    await settle();
    const tree = screen.getByRole("region", { name: "Projects and sessions" });
    expect(tree.querySelector('.rail-project [data-session-id="s-new"]')).not.toBeNull();
  });

  it("draws a session no read lists at the end, once the reads have answered", async () => {
    const tree = await unknownAtFirst(false);
    expect(tree.querySelector(':scope > .rail-tiles [data-session-id="fresh"]')).not.toBeNull();
  });
});

// A new workspace's base branch (FLOW-NEW-SESSION-R21, R28, R29): App reads the project's branches with `branch.list` and
// creates the workspace from the one the workspace chip shows.
describe("the base branch App gives a new workspace", () => {
  const branches = [
    { name: "feature/rail", default: false },
    { name: "main", default: true },
    { name: "release/1.0", default: false },
  ];
  const starting = {
    "branch.list": () => branches,
    "workspace.create": (params: { project_id: string }) => ({ ...root(params.project_id), id: "w-new", kind: "isolated", vcs_ref: "ersa/new" }),
    "session.create": (params: { workspace_id: string }) => ({ ...session("s-new", "alpha", "2026-11-03T09:00:00Z"), workspace_id: params.workspace_id }),
    "session.send": () => ({ turn: 1 }),
    "settings.set": () => ({}),
  };
  const workspaceChip = () => screen.getByRole("button", { name: /^Workspace:/ });
  const send = async (text: string) => {
    fireEvent.change(words(), { target: { value: text } });
    fireEvent.keyDown(words(), { key: "Enter" });
    await settle();
    await settle();
  };

  // verifies: FLOW-NEW-SESSION-R21
  // verifies: FLOW-NEW-SESSION-R29
  it("starts at the default branch, narrows the branches by the search box, and creates the workspace from the one chosen", async () => {
    const core = await screenWith([project("alpha")], undefined, starting);
    expect(paramsOf(core, "branch.list")).toEqual({ project_id: "alpha" });
    expect(workspaceChip().getAttribute("aria-label")).toBe("Workspace: New workspace from main");

    // Opened, the list offers both workspaces with what each means, then the base branch, with focus in its search box.
    fireEvent.click(workspaceChip());
    const list = screen.getByRole("listbox", { name: "Workspace" });
    expect(within(list).getByRole("option", { name: /New workspace/ }).textContent).toContain("Isolated");
    expect(within(list).getByRole("option", { name: /Project folder/ }).textContent).toContain("Shared");
    const search = within(list).getByRole("searchbox", { name: "Search branches" });
    expect(document.activeElement).toBe(search);
    const base = within(list).getByRole("group", { name: "Base branch" });
    expect(within(base).getAllByRole("option").map((o) => o.textContent)).toEqual(["feature/rail", "mainDefault", "release/1.0"]);
    expect(within(base).getByRole("option", { name: /main/ }).getAttribute("aria-selected")).toBe("true");

    fireEvent.change(search, { target: { value: "REL" } });
    expect(within(base).getAllByRole("option").map((o) => o.textContent)).toEqual(["release/1.0"]);
    fireEvent.change(search, { target: { value: "nothing" } });
    expect(within(base).queryAllByRole("option")).toEqual([]);
    expect(list.textContent).toContain("No branch matches.");
    fireEvent.change(search, { target: { value: "rel" } });
    fireEvent.click(within(base).getByRole("option", { name: "release/1.0" }));
    expect(workspaceChip().getAttribute("aria-label")).toBe("Workspace: New workspace from release/1.0");

    await send("Fix the build");
    expect(paramsOf(core, "workspace.create")).toEqual({ project_id: "alpha", base: "release/1.0" });
  });

  // verifies: FLOW-NEW-SESSION-R21
  it("chooses the first match with Enter in the search box", async () => {
    const core = await screenWith([project("alpha")], undefined, starting);
    fireEvent.click(workspaceChip());
    const search = screen.getByRole("searchbox", { name: "Search branches" });
    fireEvent.change(search, { target: { value: "feat" } });
    fireEvent.keyDown(search, { key: "Enter" });
    expect(workspaceChip().getAttribute("aria-label")).toBe("Workspace: New workspace from feature/rail");
    expect(document.activeElement).toBe(workspaceChip());
    await send("Fix the build");
    expect(paramsOf(core, "workspace.create")).toEqual({ project_id: "alpha", base: "feature/rail" });
  });

  it("opens with the search box empty each time", async () => {
    await screenWith([project("alpha")], undefined, starting);
    fireEvent.click(workspaceChip());
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "rel" } });
    fireEvent.keyDown(screen.getByRole("searchbox"), { key: "Escape" });
    fireEvent.click(workspaceChip());
    expect((screen.getByRole("searchbox") as HTMLInputElement).value).toBe("");
    expect(within(screen.getByRole("group", { name: "Base branch" })).getAllByRole("option")).toHaveLength(3);
  });

  // verifies: FLOW-NEW-SESSION-R29
  it("names the project folder when it is chosen, and offers no base branch for it", async () => {
    const core = await screenWith([project("alpha")], undefined, starting);
    fireEvent.click(workspaceChip());
    fireEvent.click(screen.getByRole("option", { name: /Project folder/ }));
    expect(workspaceChip().getAttribute("aria-label")).toBe("Workspace: Project folder");
    fireEvent.click(workspaceChip());
    expect(screen.queryByRole("searchbox")).toBeNull();
    await send("What does this do?");
    expect(methods(core)).not.toContain("workspace.create");
  });

  // verifies: FLOW-NEW-SESSION-R28
  it("says while the branches are read, with nothing to search, and leaves the base to Core's default", async () => {
    // Core has not answered branch.list yet.
    const core = await screenWith([project("alpha")], undefined, { ...starting, "branch.list": () => new Promise(() => {}) });
    expect(workspaceChip().getAttribute("aria-label")).toBe("Workspace: New workspace");
    fireEvent.click(workspaceChip());
    const list = screen.getByRole("listbox", { name: "Workspace" });
    expect(list.querySelector(".chip-section-note")?.textContent).toBe("Reading the branches…");
    expect(list.querySelector(".chip-section-note .glyph")).toBeNull();
    expect((within(list).getByRole("searchbox") as HTMLInputElement).disabled).toBe(true);
    // Focus goes to the chosen option, since the search box cannot be used.
    expect(document.activeElement).toBe(within(list).getByRole("option", { name: /New workspace/ }));
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    await send("Fix the build");
    expect(paramsOf(core, "workspace.create")).toEqual({ project_id: "alpha" });
  });

  it("grows to hold the branches when they arrive while the list is open, and the search box can then be used", async () => {
    let answer: (listed: typeof branches) => void = () => {};
    await screenWith([project("alpha")], undefined, { ...starting, "branch.list": () => new Promise((r) => (answer = r)) });
    fireEvent.click(workspaceChip());
    const group = () => screen.getByRole("group", { name: "Base branch" });
    expect(group().hasAttribute("data-grow")).toBe(false);
    await act(async () => answer(branches));
    await settle();
    expect(group().hasAttribute("data-grow")).toBe(true);
    expect(within(group()).getAllByRole("option")).toHaveLength(3);
    expect((screen.getByRole("searchbox") as HTMLInputElement).disabled).toBe(false);
    expect(workspaceChip().getAttribute("aria-label")).toBe("Workspace: New workspace from main");
  });

  // verifies: FLOW-NEW-SESSION-R28
  it("says the branches could not be read, and leaves the base to Core's default", async () => {
    const core = await screenWith([project("alpha")], undefined, {
      ...starting,
      "branch.list": () => {
        throw { kind: "rpc", error: { code: -32000, message: "git is not installed", data: { kind: "invalid_state" } } };
      },
    });
    expect(workspaceChip().getAttribute("aria-label")).toBe("Workspace: New workspace");
    fireEvent.click(workspaceChip());
    const list = screen.getByRole("listbox", { name: "Workspace" });
    const note = list.querySelector(".chip-section-note");
    expect(note?.textContent).toBe("Ersa couldn't read this project's branches. The new workspace will start at the default branch.");
    expect(note?.querySelector('.glyph[data-tone="danger"]')).not.toBeNull();
    expect((within(list).getByRole("searchbox") as HTMLInputElement).disabled).toBe(true);
    expect(document.activeElement).toBe(within(list).getByRole("option", { name: /New workspace/ }));
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    await send("Fix the build");
    expect(paramsOf(core, "workspace.create")).toEqual({ project_id: "alpha" });
  });

  // verifies: FLOW-NEW-SESSION-R21
  it("starts every new session at the default branch, the base chosen for the last one forgotten", async () => {
    const core = await screenWith([project("alpha")], undefined, starting);
    fireEvent.click(workspaceChip());
    fireEvent.click(screen.getByRole("option", { name: "release/1.0" }));
    await send("Fix the build");
    expect(paramsOf(core, "workspace.create")).toEqual({ project_id: "alpha", base: "release/1.0" });
    expect(JSON.stringify(paramsOf(core, "settings.set"))).not.toContain("release/1.0");

    fireEvent.click(screen.getByRole("button", { name: "New session in alpha" }));
    await settle();
    expect(workspaceChip().getAttribute("aria-label")).toBe("Workspace: New workspace from main");
  });
});

// LAY-SHELL, The rail (New session, Collapsing): a layout brief, so no requirement IDs.
describe("the rail's New session and collapsing", () => {
  const rail = () => screen.getByRole("navigation", { name: "Rail" });
  const toggle = (name: string) => within(rail()).getByRole("button", { name: new RegExp(`^${name}`) });

  it("offers New session at the rail's top once Ersa has connected, and opens the new-session screen", async () => {
    render(createElement(App, { client: fakeCore().client }));
    const top = within(rail()).getByRole("button", { name: "New session" });
    expect((top as HTMLButtonElement).disabled).toBe(true);
    await settle();
    await settle();
    expect((top as HTMLButtonElement).disabled).toBe(false);
    // The newest session shows at launch; New session leaves it for the screen.
    expect(screen.queryByRole("region", { name: "New session" })).toBeNull();
    fireEvent.click(top);
    expect(screen.getByRole("region", { name: "New session" })).toBeTruthy();
  });

  it("collapses a heading's sessions and expands them again, saying which it is, and keeps it across a reconnect", async () => {
    const core = fakeCore({}, { reattach: true });
    render(createElement(App, { client: core.client }));
    await settle();
    await settle();
    expect(toggle("beta").getAttribute("aria-expanded")).toBe("true");
    fireEvent.click(toggle("beta"));
    expect(toggle("beta").getAttribute("aria-expanded")).toBe("false");
    // Collapsed, it says how many sessions it holds, and its tiles take no focus.
    expect(toggle("beta").textContent).toContain("1 session");
    const tiles = document.getElementById(toggle("beta").getAttribute("aria-controls") ?? "");
    expect(tiles?.hasAttribute("inert")).toBe(true);

    // Ersa loses Core and reaches it again: the tree is read afresh, and beta is still collapsed.
    core.drop();
    expect(screen.queryByRole("button", { name: /^beta/ })).toBeNull();
    await act(() => new Promise((r) => setTimeout(r, 700)));
    await settle();
    await settle();
    expect(toggle("beta").getAttribute("aria-expanded")).toBe("false");
    expect(toggle("alpha").getAttribute("aria-expanded")).toBe("true");

    fireEvent.click(toggle("beta"));
    expect(toggle("beta").getAttribute("aria-expanded")).toBe("true");
    expect(toggle("beta").closest(".rail-project")?.hasAttribute("data-collapsed")).toBe(false);
  });
});

// FLOW-ORGANIZE-SESSIONS (steps 4, 6 to 8; Nothing to show): the arrange menu, kept in Core's settings.
describe("arranging the rail", () => {
  const rail = () => screen.getByRole("navigation", { name: "Rail" });
  const toggle = (name: string) => within(rail()).getByRole("button", { name: new RegExp(`^${name}`) });
  const headings = () => [...rail().querySelectorAll(".rail-heading-name")].filter((h) => !h.closest("[inert]")).map((h) => h.textContent);
  const tiles = () => [...rail().querySelectorAll<HTMLElement>("[data-session-id]")].map((el) => el.dataset.sessionId);
  const sets = (core: ReturnType<typeof fakeCore>) => core.calls.filter((c) => c.method === "settings.set").map((c) => (c.params as { patch: unknown }).patch);
  // Opens the arrange menu, then a submenu, and chooses one of its options.
  const choose = (submenu: string, option: string) => {
    fireEvent.click(within(rail()).getByRole("button", { name: "Arrange sessions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: new RegExp(`^${submenu}`) }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: option }));
  };
  const reset = () => {
    fireEvent.click(within(rail()).getByRole("button", { name: "Arrange sessions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Reset to defaults" }));
  };
  async function open(more: Parameters<typeof fakeCore>[0] = {}) {
    const core = fakeCore(more, { reattach: true });
    render(createElement(App, { client: core.client }));
    await settle();
    await settle();
    return core;
  }
  async function reconnect(core: ReturnType<typeof fakeCore>) {
    core.drop();
    await act(() => new Promise((r) => setTimeout(r, 700)));
    await settle();
    await settle();
  }
  // Today is 2 November 2026, local time, so s2 was made today and s1 yesterday.
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date(2026, 10, 2, 15));
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R13
  it("groups as chosen in the arrange menu, which names the choice, and still does after Ersa reconnects to Core", async () => {
    const core = await open({ "session.list": () => [session("s1", "alpha", new Date(2026, 10, 1, 9).toISOString()), session("s2", "beta", new Date(2026, 10, 2, 9).toISOString())] });
    expect(headings()).toEqual(["alpha", "beta", "gamma"]);
    fireEvent.click(within(rail()).getByRole("button", { name: "Arrange sessions" }));
    expect(screen.getByRole("menuitem", { name: /^Group by/ }).textContent).toContain("Project");
    fireEvent.click(screen.getByRole("menuitem", { name: /^Group by/ }));
    expect(screen.getByRole("menuitemradio", { name: "Project" }).getAttribute("aria-checked")).toBe("true");
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Date" }));
    expect(headings()).toEqual(["Today", "Yesterday"]);
    expect(tiles()).toEqual(["s2", "s1"]);
    expect(sets(core)).toEqual([{ rail: { group_by: "date" } }]);

    await reconnect(core);
    expect(headings()).toEqual(["Today", "Yesterday"]);
    choose("Group by", "None");
    expect(headings()).toEqual([]);
    expect(tiles()).toEqual(["s2", "s1"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R12
  it("keeps a collapsed group to the grouping it was collapsed in", async () => {
    const core = await open();
    fireEvent.click(toggle("alpha"));
    choose("Group by", "Date");
    expect(toggle("Yesterday").getAttribute("aria-expanded")).toBe("true");
    fireEvent.click(toggle("Today"));
    choose("Group by", "Project");
    expect(toggle("alpha").getAttribute("aria-expanded")).toBe("false");
    expect(toggle("beta").getAttribute("aria-expanded")).toBe("true");
    choose("Group by", "Date");
    expect(toggle("Today").getAttribute("aria-expanded")).toBe("false");
    expect(sets(core)).toContainEqual({ rail: { collapsed: { project: ["alpha"] } } });
    expect(sets(core)).toContainEqual({ rail: { collapsed: { date: ["today"] } } });
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R14
  it("resets the grouping, the filter and every collapsed group, and keeps the pins", async () => {
    const pinned = { ...session("s1", "alpha", "2026-11-01T09:00:00Z"), pinned: true };
    const core = await open({ "session.list": () => [pinned, session("s2", "beta", "2026-11-02T09:00:00Z")] });
    fireEvent.click(toggle("beta"));
    choose("Show", "All");
    choose("Group by", "None");
    reset();
    await settle();
    expect(sets(core).at(-1)).toEqual({ rail: { group_by: null, sort_by: null, show: null, collapsed: null } });
    expect(headings()).toEqual(["Pinned", "alpha", "beta", "gamma"]);
    expect(toggle("beta").getAttribute("aria-expanded")).toBe("true");
    expect(tiles()).toEqual(["s1", "s2"]);
    fireEvent.click(within(rail()).getByRole("button", { name: "Arrange sessions" }));
    expect(screen.getByRole("menuitem", { name: /^Show/ }).textContent).toContain("Active");
    expect(core.calls.map((c) => c.method)).not.toContain("session.arrange");
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R11
  it("shows active, archived or all sessions, never closing the active session, and says when there are no active ones", async () => {
    const gone = { ...session("s1", "alpha", "2026-11-01T09:00:00Z"), archived: true };
    const listed = [gone, session("s2", "beta", "2026-11-02T09:00:00Z")];
    await open({ "session.list": () => listed });
    expect(tiles()).toEqual(["s2"]);
    expect(screen.getByRole("region", { name: /transcript/i })).toBeTruthy();
    choose("Show", "Archived");
    expect(tiles()).toEqual(["s1"]);
    // The active session stays active, though its tile left the rail.
    expect(screen.queryByRole("region", { name: "New session" })).toBeNull();
    choose("Show", "All");
    // Each under its project: alpha's first, as Core lists the projects.
    expect(tiles()).toEqual(["s1", "s2"]);
    expect(rail().querySelector('[data-session-id="s1"] [aria-label="Archived"]')).not.toBeNull();

    cleanup();

    // Every session archived: showing active ones, the rail says there are none, beside the projects' headings.
    await open({ "session.list": () => listed.map((s) => ({ ...s, archived: true })) });
    expect(within(rail()).getByText("No active sessions.")).toBeTruthy();
    expect(headings()).toEqual(["alpha", "beta", "gamma"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R21
  it.each([
    ["active", "Archived", [{ rail: { show: "archived" } }, { rail: { show: "active" } }]],
    ["archived", "All", [{ rail: { show: "archived" } }, { rail: { show: "all" } }]],
  ] as const)("goes by what is shown when a session has started: from %s, %s chosen while it starts", async (from, chosen, expected) => {
    let created = () => {};
    const gate = new Promise<void>((resolve) => (created = resolve));
    const core = await open({
      "backend.list": () => [claude],
      "branch.list": () => [{ name: "main", default: true }],
      "workspace.create": (params: { project_id: string }) => ({ ...root(params.project_id), id: "w-new", kind: "isolated", vcs_ref: "ersa/new" }),
      "session.create": async (params: { workspace_id: string }) => {
        await gate;
        return { ...session("s-new", "beta", "2026-11-02T14:00:00Z"), workspace_id: params.workspace_id };
      },
      "session.send": () => ({ turn: 1 }),
    });
    if (from === "archived") choose("Show", "Archived");
    fireEvent.click(within(rail()).getByRole("button", { name: "New session" }));
    fireEvent.change(words(), { target: { value: "Fix the build" } });
    fireEvent.keyDown(words(), { key: "Enter" });
    await settle();
    await settle();
    expect(methods(core)).toContain("session.create");
    choose("Show", chosen);
    await settle();
    created();
    await settle();
    await settle();
    expect(sets(core).filter((p) => JSON.stringify(p).includes("show"))).toEqual(expected);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R21
  it("switches to showing active sessions when a session starts while it shows archived ones only", async () => {
    const core = await open({
      "backend.list": () => [claude],
      "branch.list": () => [{ name: "main", default: true }],
      "workspace.create": (params: { project_id: string }) => ({ ...root(params.project_id), id: "w-new", kind: "isolated", vcs_ref: "ersa/new" }),
      "session.create": (params: { workspace_id: string }) => ({ ...session("s-new", "beta", "2026-11-02T14:00:00Z"), workspace_id: params.workspace_id }),
      "session.send": () => ({ turn: 1 }),
    });
    choose("Show", "Archived");
    fireEvent.click(within(rail()).getByRole("button", { name: "New session" }));
    fireEvent.change(words(), { target: { value: "Fix the build" } });
    fireEvent.keyDown(words(), { key: "Enter" });
    await settle();
    await settle();
    expect(methods(core)).toContain("session.create");
    expect(sets(core).filter((p) => JSON.stringify(p).includes("show"))).toEqual([{ rail: { show: "archived" } }, { rail: { show: "active" } }]);
    fireEvent.click(within(rail()).getByRole("button", { name: "Arrange sessions" }));
    expect(screen.getByRole("menuitem", { name: /^Show/ }).textContent).toContain("Active");
  });
});

// FLOW-ORGANIZE-SESSIONS (step 2; Variations and errors); docs/design/RAIL.md (A tile: More; Menus).
describe("a tile's menu, and renaming on the tile", () => {
  const titled = sessions.map((s, i) => ({ ...s, title: i === 0 ? "Fix the build" : "Write the notes" }));
  const rail = () => screen.getByRole("navigation", { name: "Rail" });
  // A tile is named by its backend first, then its title; More is named for it too, so the tile is found by its class.
  const tile = (title: string) => [...rail().querySelectorAll<HTMLElement>(".session")].find((el) => el.textContent?.includes(title)) as HTMLElement;
  const field = () => screen.queryByRole("textbox", { name: "Session title" }) as HTMLInputElement | null;
  const renames = (core: ReturnType<typeof fakeCore>) => core.calls.filter((c) => c.method === "session.set_title").map((c) => c.params);
  async function open(more: Parameters<typeof fakeCore>[0] = {}) {
    const core = fakeCore({ "session.list": () => titled, ...more });
    render(createElement(App, { client: core.client }));
    await settle();
    await settle();
    return core;
  }

  // verifies: FLOW-ORGANIZE-SESSIONS-R02
  it("renames the session through session.set_title when Rename is chosen and Enter pressed", async () => {
    const core = await open({ "session.set_title": (params: { title: string }) => ({ title: params.title }) });
    fireEvent.click(within(rail()).getByRole("button", { name: "More for Fix the build" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /^Rename/ }));
    expect(field()?.value).toBe("Fix the build");
    expect(document.activeElement).toBe(field());
    fireEvent.change(field() as HTMLInputElement, { target: { value: "Fix the flaky build" } });
    fireEvent.keyDown(field() as HTMLInputElement, { key: "Enter" });
    await settle();
    expect(renames(core)).toEqual([{ id: "s1", title: "Fix the flaky build" }]);
    expect(field()).toBeNull();
  });

  it("turns the title into a field with F2, and Escape restores it, sending nothing", async () => {
    const core = await open();
    fireEvent.keyDown(tile("Write the notes"), { key: "F2" });
    fireEvent.change(field() as HTMLInputElement, { target: { value: "Something else" } });
    fireEvent.keyDown(field() as HTMLInputElement, { key: "Escape" });
    expect(field()).toBeNull();
    expect(tile("Write the notes")).toBeTruthy();
    expect(renames(core)).toEqual([]);
  });

  it("keeps the field open with the reason when the backend refuses the title", async () => {
    await open({
      "session.set_title": () => {
        // Core refuses a session whose record is gone with invalid_state (PROTO-CORE-API, Methods).
        throw { kind: "rpc", error: { code: -32010, message: "session.set_title: Claude Code has no record of this session any more." } };
      },
    });
    fireEvent.keyDown(tile("Fix the build"), { key: "F2" });
    fireEvent.change(field() as HTMLInputElement, { target: { value: "Fix it" } });
    fireEvent.keyDown(field() as HTMLInputElement, { key: "Enter" });
    await settle();
    expect(field()?.value).toBe("Fix it");
    // Said beneath the tile, which the field points to.
    const reason = document.getElementById(field()?.getAttribute("aria-errormessage") ?? "");
    expect(reason?.getAttribute("role")).toBe("alert");
    expect(reason?.textContent).toContain("no record of this session");
  });

  it("opens the same menu by right-click, the context-menu key and Shift+F10, and gives focus back to the tile", async () => {
    await open();
    const menu = () => screen.queryByRole("menu", { name: "More for Fix the build" });
    fireEvent.contextMenu(tile("Fix the build"), { clientX: 40, clientY: 80 });
    expect(menu()).toBeTruthy();
    // From the pointer, focus goes to the menu.
    expect(document.activeElement).toBe(menu());
    fireEvent.keyDown(menu() as HTMLElement, { key: "Escape" });
    expect(document.activeElement).toBe(tile("Fix the build"));
    // It fades out where it was, not under More.
    expect(menu()?.getAttribute("data-place")).toBe("point");
    await settle();

    for (const press of [{ key: "ContextMenu" }, { key: "F10", shiftKey: true }]) {
      fireEvent.keyDown(tile("Fix the build"), press);
      // The key may send its own contextmenu event after the keydown: the menu stays at the tile, on its first item.
      fireEvent.contextMenu(tile("Fix the build"), { clientX: 0, clientY: 0 });
      expect(menu()?.getAttribute("data-place")).toBe("below");
      // From the keyboard, focus goes to the first item, Pin (LAY-SHELL, A tile's menu).
      expect(document.activeElement?.textContent).toBe("Pin");
      fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
      expect(document.activeElement).toBe(tile("Fix the build"));
    }
  });

  it("keeps the field while a rename is on its way, even on Escape, so a refusal still shows its reason", async () => {
    let refuse: (error: unknown) => void = () => {};
    const core = await open({ "session.set_title": () => new Promise((_, reject) => (refuse = reject)) });
    fireEvent.keyDown(tile("Fix the build"), { key: "F2" });
    fireEvent.change(field() as HTMLInputElement, { target: { value: "Fix it" } });
    fireEvent.keyDown(field() as HTMLInputElement, { key: "Enter" });
    await settle();
    expect(renames(core)).toEqual([{ id: "s1", title: "Fix it" }]);
    fireEvent.keyDown(field() as HTMLInputElement, { key: "Escape" });
    expect(field()?.value).toBe("Fix it");
    await act(async () => refuse({ kind: "rpc", error: { code: -32010, message: "session.set_title: Claude Code has no record of this session any more." } }));
    await settle();
    const reason = document.getElementById(field()?.getAttribute("aria-errormessage") ?? "");
    expect(reason?.textContent).toContain("no record of this session");
  });

  it("offers Rename on a starting session, and shows Core's refusal in the field until the backend has named it", async () => {
    const starting = titled.map((s, i) => (i === 0 ? { ...s, state: "starting" as const, tile: { state: "starting" as const, pending: [] } } : s));
    const core = await open({
      "session.list": () => starting,
      "session.set_title": () => {
        // Core refuses a session its backend has not named yet with invalid_state (PROTO-CORE-API, Methods).
        throw { kind: "rpc", error: { code: -32010, message: "session.set_title: the backend has not named this session yet" } };
      },
    });
    fireEvent.click(within(rail()).getByRole("button", { name: "More for Fix the build" }));
    const item = screen.getByRole("menuitem", { name: /^Rename/ });
    expect(item.getAttribute("aria-disabled")).toBeNull();
    fireEvent.click(item);
    fireEvent.change(field() as HTMLInputElement, { target: { value: "Fix it" } });
    fireEvent.keyDown(field() as HTMLInputElement, { key: "Enter" });
    await settle();
    expect(renames(core)).toEqual([{ id: "s1", title: "Fix it" }]);
    expect(field()?.value).toBe("Fix it");
    const reason = document.getElementById(field()?.getAttribute("aria-errormessage") ?? "");
    expect(reason?.textContent).toContain("has not named this session yet");
  });
});

// Archiving and unarchiving (FLOW-ARCHIVE-DELETE-SESSION, steps 1 to 3), against a fake Core whose list changes as
// Core's would.
describe("archiving a session from its tile, and an archived session read-only", () => {
  const rail = () => screen.getByRole("navigation", { name: "Rail" });
  const titled = (archived: { [id: string]: boolean } = {}) =>
    sessions.map((s, i) => ({ ...s, title: i === 0 ? "Fix the build" : "Write the notes", archived: archived[s.id] ?? false }));
  const tileNamed = (title: string) => [...rail().querySelectorAll<HTMLElement>(".session")].find((el) => el.textContent?.includes(title));
  const callsOf = (core: ReturnType<typeof fakeCore>, method: Method) => core.calls.filter((c) => c.method === method).map((c) => c.params);
  const composerBox = () => screen.queryByRole("textbox", { name: "Message" });
  async function open(list: { current: Session[] }, more: Parameters<typeof fakeCore>[0] = {}) {
    const core = fakeCore({ "session.list": () => list.current, ...more });
    render(createElement(App, { client: core.client }));
    await settle();
    await settle();
    return core;
  }
  function chooseArchive(title: string, item: RegExp) {
    fireEvent.click(within(rail()).getByRole("button", { name: `More for ${title}` }));
    fireEvent.click(screen.getByRole("menuitem", { name: item }));
  }

  it("archives through session.set_archived, and the tile leaves the rail once Core lists it archived", async () => {
    const list = { current: titled() };
    const core = await open(list, {
      "session.set_archived": (params: { id: string; archived: boolean }) => {
        list.current = titled({ [params.id]: params.archived });
        return list.current.find((s) => s.id === params.id);
      },
    });
    // The newest, Write the notes, is active; Fix the build is not.
    chooseArchive("Fix the build", /^Archive/);
    await settle();
    expect(callsOf(core, "session.set_archived")).toEqual([{ id: "s1", archived: true }]);
    expect(tileNamed("Fix the build")).toBeUndefined();
    expect(tileNamed("Write the notes")).toBeTruthy();
  });

  it.each([
    // The rail reads Fix the build (alpha), Write the notes (beta), Read the logs (gamma). From the middle the next tile
    // takes focus; from the last, with no next, the previous does.
    ["Write the notes", "Read the logs"],
    ["Read the logs", "Write the notes"],
  ])("moves keyboard focus from %s, archived from its menu, to %s", async (archived, next) => {
    const three = (marks: { [id: string]: boolean } = {}) => [
      ...titled(marks),
      { ...session("s3", "gamma", "2026-11-03T09:00:00Z"), title: "Read the logs", archived: marks.s3 ?? false },
    ];
    const list = { current: three() };
    await open(list, {
      "session.set_archived": (params: { id: string; archived: boolean }) => {
        list.current = three({ [params.id]: params.archived });
        return list.current.find((s) => s.id === params.id);
      },
    });
    // Fix the build is active, so the stage stays on it.
    fireEvent.click(tileNamed("Fix the build") as HTMLElement);
    await settle();
    const more = within(rail()).getByRole("button", { name: `More for ${archived}` });
    more.focus();
    chooseArchive(archived, /^Archive/);
    expect(document.activeElement).toBe(more);
    await settle();
    expect(tileNamed(archived)).toBeUndefined();
    expect(document.activeElement).toBe(tileNamed(next));
  });

  it("shows the new-session screen when the active session is archived", async () => {
    const list = { current: titled() };
    await open(list, {
      "session.set_archived": (params: { id: string; archived: boolean }) => {
        list.current = titled({ [params.id]: params.archived });
        return list.current.find((s) => s.id === params.id);
      },
    });
    expect(composerBox()).toBeTruthy();
    chooseArchive("Write the notes", /^Archive/);
    await settle();
    expect(tileNamed("Write the notes")).toBeUndefined();
    // The new-session screen's session controls name a project chip; a session's are read-only and name no host list.
    expect(screen.getByRole("button", { name: /^Project:/ })).toBeTruthy();
    expect(screen.queryByText("This session is archived. Resume it to send a message.")).toBeNull();
  });

  it("keeps the tile and says why when Core refuses to archive", async () => {
    const list = { current: titled() };
    await open(list, {
      "session.set_archived": () => {
        throw { kind: "rpc", error: { code: -32010, message: "session.set_archived: the session is being deleted" } };
      },
    });
    chooseArchive("Fix the build", /^Archive/);
    await settle();
    expect(tileNamed("Fix the build")).toBeTruthy();
    const alert = within(rail()).getByRole("alert");
    expect(alert.textContent).toContain("Ersa can't archive this session.");
    expect(alert.textContent).toContain("being deleted");
  });

  it("opens with the newest session that is not archived", async () => {
    await open({ current: titled({ s2: true }) });
    expect(tileNamed("Write the notes")).toBeUndefined();
    expect(screen.getByRole("heading", { name: "Fix the build" })).toBeTruthy();
  });

  /** The active session, Write the notes, archived elsewhere: a new session's tile makes App read the list again. */
  async function archivedWhileOpen(more: Parameters<typeof fakeCore>[0] = {}) {
    const list = { current: titled() };
    const core = await open(list, more);
    list.current = titled({ s2: true });
    core.emit({ kind: "notification", method: "session.tile", params: { session_id: "s9", tile: { state: "idle", pending: [] } } } as CoreEvent);
    await settle();
    return { core, list };
  }

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R02
  it("shows an archived session's transcript with a note and Resume in place of the composer", async () => {
    await archivedWhileOpen();
    expect(screen.getByRole("heading", { name: "Write the notes" })).toBeTruthy();
    expect(composerBox()).toBeNull();
    expect(screen.getByText("This session is archived. Resume it to send a message.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Resume" })).toBeTruthy();
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R02
  it("leaves the session archived and says why when Core refuses the resume", async () => {
    const { core } = await archivedWhileOpen({
      "session.resume": () => {
        throw { kind: "rpc", error: { code: -32010, message: "session.resume: resuming a session arrives with milestone M1" } };
      },
    });
    const resume = screen.getByRole("button", { name: "Resume" });
    resume.focus();
    fireEvent.click(resume);
    // Unavailable while Core answers, but never disabled, which would drop keyboard focus.
    expect(resume.getAttribute("aria-disabled")).toBe("true");
    expect(resume.hasAttribute("disabled")).toBe(false);
    // Pressing it again meanwhile sends nothing more.
    fireEvent.click(resume);
    await settle();
    expect(callsOf(core, "session.resume")).toEqual([{ id: "s2" }]);
    // Focus comes back to Resume once it can be pressed again.
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Resume" }));
    const alert = screen.getAllByRole("alert").find((a) => a.classList.contains("archived-problem"));
    expect(alert?.textContent).toContain("Ersa can't resume this session.");
    expect(alert?.textContent).toContain("milestone M1");
    expect(composerBox()).toBeNull();
    expect(screen.getByRole("button", { name: "Resume" })).toBeTruthy();
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R03
  it("brings the composer back once Core accepts the resume and lists the session unarchived, sending nothing else", async () => {
    const holder: { list?: { current: Session[] } } = {};
    const { core, list } = await archivedWhileOpen({
      "session.resume": () => {
        // Core clears the mark as it accepts the resume.
        if (holder.list) holder.list.current = titled();
        return {};
      },
    });
    holder.list = list;
    fireEvent.click(screen.getByRole("button", { name: "Resume" }));
    await settle();
    await settle();
    expect(composerBox()).toBeTruthy();
    // The pin and the section stay: neither session.set_archived nor session.arrange was sent.
    expect(callsOf(core, "session.set_archived")).toEqual([]);
    expect(callsOf(core, "session.arrange")).toEqual([]);
  });
});

// FLOW-ORGANIZE-SESSIONS (step 1; Variations and errors: Saving fails); docs/design/RAIL.md (A group heading).
describe("pinning a session from its tile", () => {
  const rail = () => screen.getByRole("navigation", { name: "Rail" });
  const more = (title: string) => within(rail()).getByRole("button", { name: `More for ${title}` });
  // The group a tile is in, by its heading's name; a tile is found by its title.
  const groupOf = (title: string) =>
    [...rail().querySelectorAll<HTMLElement>(".session")]
      .find((el) => el.textContent?.includes(title))
      ?.closest(".rail-project")
      ?.querySelector(".rail-heading-name")?.textContent;
  const arranges = (core: ReturnType<typeof fakeCore>) => core.calls.filter((c) => c.method === "session.arrange").map((c) => c.params);
  // Core keeps the pins: session.arrange changes what session.list answers, as it does.
  async function open(refuse?: string) {
    let listed = sessions.map((s, i) => ({ ...s, title: i === 0 ? "Fix the build" : "Write the notes" }));
    const core = fakeCore({
      "session.list": () => listed,
      "session.arrange": (params: { id: string; pinned: boolean }) => {
        if (refuse) throw { kind: "rpc", error: { code: -32010, message: refuse } };
        listed = listed.map((s) => (s.id === params.id ? { ...s, pinned: params.pinned } : s));
        return listed.find((s) => s.id === params.id);
      },
    });
    render(createElement(App, { client: core.client }));
    await settle();
    await settle();
    return core;
  }

  // verifies: FLOW-ORGANIZE-SESSIONS-R01
  it("moves a pinned tile to a leading Pinned group, and Unpin returns it to its project", async () => {
    const core = await open();
    // Nothing pinned: no Pinned group.
    expect(within(rail()).queryByRole("button", { name: /^Pinned/ })).toBeNull();
    expect(groupOf("Fix the build")).toBe("alpha");
    fireEvent.click(more("Fix the build"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Pin" }));
    await settle();
    await settle();
    expect(arranges(core)).toEqual([{ id: "s1", pinned: true }]);
    expect(groupOf("Fix the build")).toBe("Pinned");
    // It leads the rail, before every project, and its project keeps its heading.
    const headings = [...rail().querySelectorAll(".rail-heading-name")].map((el) => el.textContent);
    expect(headings).toEqual(["Pinned", "alpha", "beta", "gamma"]);
    expect(groupOf("Write the notes")).toBe("beta");

    fireEvent.click(more("Fix the build"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Unpin" }));
    await settle();
    await settle();
    expect(arranges(core)).toEqual([
      { id: "s1", pinned: true },
      { id: "s1", pinned: false },
    ]);
    expect(groupOf("Fix the build")).toBe("alpha");
    expect(within(rail()).queryByRole("button", { name: /^Pinned/ })).toBeNull();
  });

  it("gives keyboard focus to the moved tile, or to the Pinned heading while that group is collapsed", async () => {
    await open();
    const tileOf = (title: string) => [...rail().querySelectorAll<HTMLElement>(".session")].find((el) => el.textContent?.includes(title));
    tileOf("Fix the build")?.focus();
    fireEvent.keyDown(tileOf("Fix the build") as HTMLElement, { key: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "Pin" }));
    await settle();
    await settle();
    expect(groupOf("Fix the build")).toBe("Pinned");
    expect(document.activeElement).toBe(tileOf("Fix the build"));

    const pinned = within(rail()).getByRole("button", { name: /^Pinned/ });
    fireEvent.click(pinned);
    tileOf("Write the notes")?.focus();
    fireEvent.keyDown(tileOf("Write the notes") as HTMLElement, { key: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "Pin" }));
    await settle();
    await settle();
    expect(groupOf("Write the notes")).toBe("Pinned");
    expect(document.activeElement).toBe(within(rail()).getByRole("button", { name: /^Pinned/ }));
  });

  it("slides each moved tile and heading from where it was over motion-settle with motion-ease-in-out", async () => {
    await open();
    // jsdom lays nothing out: each slid element sits at its order on the page, and the motion tokens are set here.
    document.documentElement.style.setProperty("--motion-settle", "240ms");
    document.documentElement.style.setProperty("--motion-ease-in-out", "ease-in-out");
    const order = () => [...document.querySelectorAll<HTMLElement>("[data-slide]")];
    const rect = vi.spyOn(HTMLElement.prototype, "offsetTop", "get").mockImplementation(function (this: HTMLElement) {
      return order().indexOf(this) * 10;
    });
    const slid: { slide: string | undefined; keyframes: Keyframe[]; options: KeyframeAnimationOptions }[] = [];
    HTMLElement.prototype.animate = function (this: HTMLElement, keyframes: Keyframe[], options: KeyframeAnimationOptions) {
      slid.push({ slide: this.dataset.slide, keyframes, options });
      return {} as Animation;
    } as never;
    try {
      const before = order().map((el) => el.dataset.slide);
      fireEvent.click(more("Write the notes"));
      fireEvent.click(screen.getByRole("menuitem", { name: "Pin" }));
      await settle();
      await settle();
      const after = order().map((el) => el.dataset.slide);
      // Every element that had a place and moved slides from it; the Pinned heading is new, so it appears in place.
      const moved = after.filter((key) => key !== "pinned" && before.indexOf(key) !== after.indexOf(key));
      expect(moved).toContain("session:s2");
      expect(slid.map((s) => s.slide).sort()).toEqual([...moved].sort());
      const tile = slid.find((s) => s.slide === "session:s2");
      expect(tile?.keyframes).toEqual([{ transform: `translateY(${(before.indexOf("session:s2") - after.indexOf("session:s2")) * 10}px)` }, { transform: "none" }]);
      expect(tile?.options).toEqual({ duration: 240, easing: "ease-in-out" });
    } finally {
      rect.mockRestore();
      delete (HTMLElement.prototype as { animate?: unknown }).animate;
      document.documentElement.removeAttribute("style");
    }
  });

  it("slides nothing when Core refuses the pin, or when the user asked for reduced motion", async () => {
    const animate = vi.fn(() => ({}) as Animation);
    HTMLElement.prototype.animate = animate as never;
    document.documentElement.style.setProperty("--motion-settle", "240ms");
    document.documentElement.style.setProperty("--motion-ease-in-out", "ease-in-out");
    const position = vi.spyOn(HTMLElement.prototype, "offsetTop", "get").mockImplementation(function (this: HTMLElement) {
      // Each element sits at its order among every element on the page, so a note added above a tile moves it.
      return [...document.querySelectorAll("*")].indexOf(this) * 10;
    });
    const reduce = (on: boolean) => vi.stubGlobal("matchMedia", (query: string) => ({ matches: on && query.includes("reduce") }) as MediaQueryList);
    try {
      reduce(true);
      await open();
      fireEvent.click(more("Write the notes"));
      fireEvent.click(screen.getByRole("menuitem", { name: "Pin" }));
      await settle();
      await settle();
      expect(groupOf("Write the notes")).toBe("Pinned");
      expect(animate).not.toHaveBeenCalled();
      cleanup();

      reduce(false);
      await open("session.arrange: the session is being deleted");
      fireEvent.click(more("Write the notes"));
      fireEvent.click(screen.getByRole("menuitem", { name: "Pin" }));
      await settle();
      await settle();
      expect(within(rail()).getByRole("alert").textContent).toContain("the session is being deleted");
      expect(animate).not.toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
      position.mockRestore();
      delete (HTMLElement.prototype as { animate?: unknown }).animate;
      document.documentElement.removeAttribute("style");
    }
  });

  it("makes Pin and Unpin unavailable on every tile while a pin is saved", async () => {
    let answer: () => void = () => {};
    const core = fakeCore({
      "session.list": () => sessions.map((s, i) => ({ ...s, title: i === 0 ? "Fix the build" : "Write the notes" })),
      "session.arrange": (params: { id: string; pinned: boolean }) =>
        new Promise((resolve) => (answer = () => resolve({ ...sessions[0], title: "Fix the build", pinned: params.pinned }))),
    });
    render(createElement(App, { client: core.client }));
    await settle();
    await settle();
    fireEvent.click(more("Fix the build"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Pin" }));
    await settle();
    fireEvent.click(more("Write the notes"));
    const item = screen.getByRole("menuitem", { name: /^Pin/ });
    expect(item.getAttribute("aria-disabled")).toBe("true");
    expect(item.textContent).toContain("Saving…");
    await act(async () => answer());
  });

  it("collapses the Pinned group like any group", async () => {
    await open();
    fireEvent.click(more("Fix the build"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Pin" }));
    await settle();
    await settle();
    const pinned = within(rail()).getByRole("button", { name: /^Pinned/ });
    fireEvent.click(pinned);
    expect(pinned.getAttribute("aria-expanded")).toBe("false");
    expect(pinned.textContent).toContain("1 session");
    expect(document.getElementById(pinned.getAttribute("aria-controls") ?? "")?.hasAttribute("inert")).toBe(true);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R15
  it("leaves the tile where Core has it and says why when Core refuses the pin", async () => {
    const core = await open("session.arrange: the session is being deleted");
    fireEvent.click(more("Fix the build"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Pin" }));
    await settle();
    await settle();
    expect(arranges(core)).toEqual([{ id: "s1", pinned: true }]);
    expect(groupOf("Fix the build")).toBe("alpha");
    expect(within(rail()).queryByRole("button", { name: /^Pinned/ })).toBeNull();
    const alert = within(rail()).getByRole("alert");
    expect(alert.textContent).toContain("Ersa can't pin this session.");
    expect(alert.textContent).toContain("the session is being deleted");
    // Trying again takes the old reason away.
    fireEvent.click(more("Fix the build"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Pin" }));
    expect(within(rail()).queryByRole("alert")).toBeNull();
    await settle();
  });
});

// The composer while Ersa can't reach the sessions (FLOW-SEND-MESSAGE-R06): App tells it so whichever view holds it.
describe("the composer App draws when Core is lost", () => {
  // verifies: FLOW-SEND-MESSAGE-R06
  it("says Ersa can't reach the sessions and is unavailable on the new-session screen", async () => {
    // A host with a backend to run, so the box is available until Core is lost.
    const core = await screenWith([project("alpha")]);
    // Chosen with New session, the screen stays while Core is away.
    fireEvent.click(screen.getByRole("button", { name: "New session in alpha" }));
    const message = () => screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
    expect(screen.getByRole("region", { name: "New session" })).toBeTruthy();
    expect(message().disabled).toBe(false);
    core.drop();
    await settle();
    expect(screen.getByRole("region", { name: "New session" })).toBeTruthy();
    expect(document.querySelector(".composer-status-line")?.textContent).toBe("Ersa can't reach your sessions. It's trying again.");
    expect(message().disabled).toBe(true);
  });

  // verifies: FLOW-SEND-MESSAGE-R06
  it("says Ersa can't reach the sessions and is unavailable under a session", async () => {
    const core = fakeCore();
    render(createElement(App, { client: core.client }));
    await settle();
    await settle();
    // The newest session shows at launch.
    const message = () => screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
    expect(message().disabled).toBe(false);
    core.drop();
    await settle();
    expect(document.querySelector(".composer-status-line")?.textContent).toBe("Ersa can't reach your sessions. It's trying again.");
    expect(message().disabled).toBe(true);
  });
});

// Stop under a session (FLOW-SEND-MESSAGE-R03, R04): App holds stop from the send until the turn shows.
describe("stop under a session App draws", () => {
  // verifies: FLOW-SEND-MESSAGE-R03
  // verifies: FLOW-SEND-MESSAGE-R04
  it("keeps stop shown and Enter from sending when Core answers the send before the turn shows", async () => {
    const core = fakeCore({
      "session.subscribe": () => ({ last_seq: -1 }),
      "session.send": () => ({ turn: 1 }),
    });
    render(createElement(App, { client: core.client }));
    await settle();
    await settle();
    // The newest session shows at launch; its turn.started never arrives.
    const message = () => screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
    fireEvent.change(message(), { target: { value: "Fix the build" } });
    await act(async () => {
      fireEvent.keyDown(message(), { key: "Enter" });
    });
    await settle();
    const composer = screen.getByRole("form", { name: "Composer" });
    expect(within(composer).getByRole("button", { name: "Stop" })).toBeTruthy();
    fireEvent.change(message(), { target: { value: "And the tests" } });
    await act(async () => {
      fireEvent.keyDown(message(), { key: "Enter" });
    });
    await settle();
    expect(core.calls.filter((c) => c.method === "session.send")).toHaveLength(1);
    expect(message().value).toBe("And the tests");
  });
});

// Attachments under a session and on the new-session screen (FLOW-SEND-MESSAGE-R07, R10, R13, R16).
describe("the attachments App gives the composer", () => {
  const inspected: Record<string, unknown> = {
    "D:/code/beta/notes.md": { path: "D:/code/beta/notes.md", name: "notes.md", kind: "file", size_bytes: 900, lines: 12, copied: false },
    "C:/pics/shot.png": { path: "C:/pics/shot.png", name: "shot.png", kind: "image", size_bytes: 2048, media_type: "image/png", copied: false },
    // In beta's folder by its path, but a junction elsewhere: Core says sending copies it.
    "D:/code/beta/src": { path: "D:/code/beta/src", name: "src", kind: "folder", copied: true },
  };
  const inspect = (params: { host_id: string; path: string; workspace_id?: string }) => {
    // Exactly one of the workspace and the host.
    if ((params.host_id === undefined) === (params.workspace_id === undefined)) throw new Error("name one of workspace or host");
    if (params.host_id !== undefined && params.host_id !== "local") throw new Error("wrong host");
    const info = inspected[params.path] as Record<string, unknown> | undefined;
    if (!info) throw { kind: "rpc", error: { code: -32000, message: "attachment.inspect: missing", data: { kind: "not_found", detail: "This file no longer exists." } } };
    // Without a workspace Core says every file and folder is copied.
    if (params.workspace_id === undefined) return { ...info, copied: info.kind !== "image" };
    return info;
  };
  const choose = async (name: string) => {
    fireEvent.click(screen.getByRole("button", { name: "Attach" }));
    fireEvent.click(screen.getByRole("menuitem", { name }));
    await settle();
  };
  const shown = () => Array.from(document.querySelectorAll(".composer-attachment")).map((li) => li.textContent);

  // verifies: FLOW-SEND-MESSAGE-R07
  // verifies: FLOW-SEND-MESSAGE-R13
  it("opens the system's dialog for each choice, reads only the paths picked, and sends them with the message", async () => {
    const shell = fakeShell("D:/code/beta/src");
    shell.pickFiles.mockImplementation(async (images: boolean) => (images ? ["C:/pics/shot.png"] : ["D:/code/beta/notes.md"]));
    const core = fakeCore({ "session.subscribe": () => ({ last_seq: -1 }), "session.send": () => ({ turn: 1 }), "attachment.inspect": inspect });
    render(createElement(App, { client: core.client, shell }));
    await settle();
    await settle();
    await choose("Add files…");
    await choose("Add images…");
    await choose("Add a folder…");
    expect(shell.pickFiles.mock.calls).toEqual([[false], [true]]);
    expect(shell.pickFolder).toHaveBeenCalledTimes(1);
    expect(core.calls.filter((c) => c.method === "attachment.inspect").map((c) => c.params)).toEqual([
      { path: "D:/code/beta/notes.md", workspace_id: "w-beta" },
      { path: "C:/pics/shot.png", workspace_id: "w-beta" },
      { path: "D:/code/beta/src", workspace_id: "w-beta" },
    ]);
    // R17: the note follows what Core says for the session's workspace; an image goes inline.
    expect(shown()).toEqual(["notes.md12 lines", "shot.png2 KB", "srcFolderWill be copied into the workspace's .ersa folder"]);
    fireEvent.change(words(), { target: { value: "Look" } });
    await act(async () => {
      fireEvent.keyDown(words(), { key: "Enter" });
    });
    await settle();
    expect(paramsOf(core, "session.send")).toEqual({
      id: "s2",
      text: "Look",
      attachments: [
        { kind: "file", path: "D:/code/beta/notes.md" },
        { kind: "image", path: "C:/pics/shot.png" },
        { kind: "folder", path: "D:/code/beta/src" },
      ],
    });
  });

  // verifies: FLOW-SEND-MESSAGE-R10
  it("attaches a path Core can't read marked with why, holding the message, and a canceled dialog attaches nothing", async () => {
    const shell = fakeShell();
    shell.pickFiles.mockResolvedValueOnce([]).mockResolvedValueOnce(["D:/code/beta/gone.md"]);
    const core = fakeCore({ "session.subscribe": () => ({ last_seq: -1 }), "attachment.inspect": inspect });
    render(createElement(App, { client: core.client, shell }));
    await settle();
    await settle();
    await choose("Add files…");
    expect(shown()).toEqual([]);
    await choose("Add files…");
    expect(shown()).toEqual(["gone.mdThis file no longer exists."]);
    fireEvent.change(words(), { target: { value: "Look" } });
    fireEvent.keyDown(words(), { key: "Enter" });
    await settle();
    expect(methods(core)).not.toContain("session.send");
  });

  // verifies: FLOW-SEND-MESSAGE-R10
  // verifies: PROTO-CORE-API-R44
  it("shows a folder Core refuses, as one holding the workspace, as that attachment's refusal and nothing else", async () => {
    const shell = fakeShell("D:/code");
    const holds = { kind: "rpc", error: { code: -32000, message: "attachment.inspect: D:/code holds the session's workspace", data: { kind: "attachment_refused" } } };
    const core = fakeCore({ "session.subscribe": () => ({ last_seq: -1 }), "attachment.inspect": () => Promise.reject(holds) });
    render(createElement(App, { client: core.client, shell }));
    await settle();
    await settle();
    await choose("Add a folder…");
    expect(shown()).toEqual(["codeD:/code holds the session's workspace"]);
    expect(screen.getAllByRole("alert").map((a) => a.textContent)).toEqual(["D:/code holds the session's workspace"]);
    expect(document.querySelector(".composer-status-line")).toBeNull();
  });

  // verifies: FLOW-SEND-MESSAGE-R08
  // verifies: FLOW-SEND-MESSAGE-R13
  it("reads the files dropped on the box as the dialog's, for the session's workspace, and attaches them", async () => {
    const shell = fakeShell();
    let emit: (event: DragDrop) => void = () => {};
    shell.onDragDrop.mockImplementation((handler) => ((emit = handler), () => {}));
    vi.spyOn(HTMLFormElement.prototype, "getBoundingClientRect").mockReturnValue({ left: 0, top: 100, right: 400, bottom: 200 } as DOMRect);
    const core = fakeCore({ "session.subscribe": () => ({ last_seq: -1 }), "attachment.inspect": inspect });
    render(createElement(App, { client: core.client, shell }));
    await settle();
    await settle();
    act(() => emit({ type: "over", x: 10, y: 150 }));
    expect(screen.getByRole("form", { name: "Composer" }).hasAttribute("data-drag-over")).toBe(true);
    act(() => emit({ type: "drop", x: 10, y: 150, paths: ["D:/code/beta/notes.md", "D:/code/beta/src"] }));
    await settle();
    expect(core.calls.filter((c) => c.method === "attachment.inspect").map((c) => c.params)).toEqual([
      { path: "D:/code/beta/notes.md", workspace_id: "w-beta" },
      { path: "D:/code/beta/src", workspace_id: "w-beta" },
    ]);
    expect(shown()).toEqual(["notes.md12 lines", "srcFolderWill be copied into the workspace's .ersa folder"]);
    vi.restoreAllMocks();
  });

  // verifies: FLOW-SEND-MESSAGE-R08
  it("takes files dropped on the new-session screen's box, read on its host for a new workspace", async () => {
    const shell = fakeShell();
    let emit: (event: DragDrop) => void = () => {};
    shell.onDragDrop.mockImplementation((handler) => ((emit = handler), () => {}));
    vi.spyOn(HTMLFormElement.prototype, "getBoundingClientRect").mockReturnValue({ left: 0, top: 100, right: 400, bottom: 200 } as DOMRect);
    const core = await screenWith([project("alpha")], shell, {
      "attachment.inspect": (params: { path: string }) => ({ path: params.path, name: "notes.md", kind: "file", size_bytes: 9, lines: 1, copied: true }),
    });
    fireEvent.click(screen.getByRole("button", { name: /^Workspace:/ }));
    fireEvent.click(screen.getByRole("option", { name: /New workspace/ }));
    act(() => emit({ type: "drop", x: 10, y: 150, paths: ["D:/code/alpha/notes.md"] }));
    await settle();
    expect(core.calls.filter((c) => c.method === "attachment.inspect").map((c) => c.params)).toEqual([{ host_id: "local", path: "D:/code/alpha/notes.md" }]);
    expect(shown()).toEqual(["notes.md1 lineWill be copied into the workspace's .ersa folder"]);
    vi.restoreAllMocks();
  });

  // verifies: FLOW-SEND-MESSAGE-R16
  it("shows where a session's message is, and where a new session's first message is", async () => {
    const core = fakeCore({ "session.subscribe": () => ({ last_seq: -1 }), "session.send": () => new Promise(() => {}) });
    render(createElement(App, { client: core.client }));
    await settle();
    await settle();
    fireEvent.change(words(), { target: { value: "Look" } });
    fireEvent.keyDown(words(), { key: "Enter" });
    await settle();
    core.emit({ kind: "notification", method: "session.send_progress", params: { session_id: "s2", phase: "copying", copied_bytes: 1, total_bytes: 2 } });
    expect(document.querySelector(".composer-status-line")?.textContent).toBe("Copying attachments, 50 percent done…");
    cleanup();

    const started = await screenWith([project("alpha")], undefined, {
      "workspace.create": (params: { project_id: string }) => ({ ...root(params.project_id), id: "w-new", kind: "isolated", vcs_ref: "ersa/new" }),
      "session.create": () => session("s-new", "alpha", "2026-11-03T09:00:00Z"),
      "session.send": () => new Promise(() => {}),
    });
    fireEvent.change(words(), { target: { value: "Fix it" } });
    fireEvent.keyDown(words(), { key: "Enter" });
    await settle();
    await settle();
    started.emit({ kind: "notification", method: "session.send_progress", params: { session_id: "s-new", phase: "starting" } });
    expect(document.querySelector(".composer-status-line")?.textContent).toBe("Starting the backend…");
  });
});

// A new session's first message with attachments (FLOW-SEND-MESSAGE-R10, R13 to R15).
describe("the attachments of a new session's first message", () => {
  const notes = { path: "D:/code/alpha/notes.md", name: "notes.md", kind: "file", size_bytes: 900, lines: 12, copied: true };
  const start = (send: () => unknown) => {
    // Core lists the session once it is created, alive and idle.
    let created = false;
    const made = { ...session("s-new", "alpha", "2026-11-03T09:00:00Z"), workspace_id: "w-new", state: "idle" as const };
    return {
      // Judged against the project's root workspace: inside it.
      "attachment.inspect": (params: { workspace_id?: string }) => (params.workspace_id === "w-alpha" ? { ...notes, copied: false } : notes),
      "workspace.create": (params: { project_id: string }) => ({ ...root(params.project_id), id: "w-new", kind: "isolated", path: "D:/ws/alpha-1", vcs_ref: "ersa/new" }),
      "session.create": () => ((created = true), made),
      "session.subscribe": () => ({ last_seq: -1 }),
      "settings.set": () => ({}),
      "session.send": send,
      "session.list": () => (created ? [made] : []),
    };
  };
  const attachNotes = async () => {
    fireEvent.click(screen.getByRole("button", { name: "Attach", exact: true } as never));
    fireEvent.click(screen.getByRole("menuitem", { name: "Add files…" }));
    await settle();
  };
  const shown = () => Array.from(document.querySelectorAll(".composer-attachment")).map((li) => li.textContent);
  const sendIt = async () => {
    // Started in a new workspace, which the first message makes.
    fireEvent.click(screen.getByRole("button", { name: /^Workspace:/ }));
    fireEvent.click(screen.getByRole("option", { name: /New workspace/ }));
    fireEvent.change(words(), { target: { value: "Look" } });
    fireEvent.keyDown(words(), { key: "Enter" });
    await settle();
    await settle();
    await settle();
  };
  const shellWith = () => {
    const shell = fakeShell();
    shell.pickFiles.mockResolvedValue(["D:/code/alpha/notes.md"]);
    return shell;
  };

  // verifies: FLOW-SEND-MESSAGE-R17
  it("asks Core about the project folder's workspace when it is chosen, and about none for a new workspace", async () => {
    const core = await screenWith([project("alpha")], shellWith(), start(() => ({ turn: 1 })));
    const choose = (name: RegExp) => {
      fireEvent.click(screen.getByRole("button", { name: /^Workspace:/ }));
      fireEvent.click(screen.getByRole("option", { name }));
    };
    choose(/Project folder/);
    await attachNotes();
    expect(shown()).toEqual(["notes.md12 lines"]);
    // Choosing a new workspace after: judged for another, it is taken to be copied.
    choose(/New workspace/);
    expect(shown()).toEqual(["notes.md12 linesWill be copied into the workspace's .ersa folder"]);
    fireEvent.click(screen.getByRole("button", { name: "Remove notes.md" }));
    await attachNotes();
    expect(shown()).toEqual(["notes.md12 linesWill be copied into the workspace's .ersa folder"]);
    expect(core.calls.filter((c) => c.method === "attachment.inspect").map((c) => c.params)).toEqual([
      { path: "D:/code/alpha/notes.md", workspace_id: "w-alpha" },
      { host_id: "local", path: "D:/code/alpha/notes.md" },
    ]);
  });

  // verifies: FLOW-SEND-MESSAGE-R13
  it("sends them with the first message", async () => {
    const core = await screenWith([project("alpha")], shellWith(), start(() => ({ turn: 1 })));
    await attachNotes();
    await sendIt();
    expect(paramsOf(core, "session.send")).toEqual({ id: "s-new", text: "Look", attachments: [{ kind: "file", path: "D:/code/alpha/notes.md" }] });
  });

  // verifies: FLOW-SEND-MESSAGE-R10
  it("opens the session with the message given back, the refused attachment marked and its reason said once", async () => {
    const refused = { kind: "rpc", error: { code: -32000, message: "session.send: refused", data: { kind: "attachment_refused", detail: "This file is too large.", index: 0 } } };
    await screenWith([project("alpha")], shellWith(), start(() => Promise.reject(refused)));
    await attachNotes();
    await sendIt();
    expect(screen.queryByRole("region", { name: "New session" })).toBeNull();
    expect(words().value).toBe("Look");
    expect(shown()).toEqual(["notes.mdThis file is too large."]);
    expect(screen.getAllByRole("alert").map((a) => a.textContent)).toEqual(["This file is too large."]);
  });

  // verifies: FLOW-SEND-MESSAGE-R15
  it("opens the session saying Ersa can't tell whether the first message arrived, the message kept", async () => {
    const lost = { kind: "unavailable", message: "Ersa got no answer in time.", sent: true };
    await screenWith([project("alpha")], shellWith(), start(() => Promise.reject(lost)));
    await attachNotes();
    await sendIt();
    expect(screen.queryByRole("region", { name: "New session" })).toBeNull();
    expect(document.querySelector(".composer-status-line")?.textContent).toBe("Ersa can't tell whether your message reached the agent. If it did, the transcript shows it.");
    expect(words().value).toBe("Look");
    expect(shown()).toEqual(["notes.md12 linesWill be copied into the workspace's .ersa folder"]);
  });
});

describe("reading the paths picked", () => {
  // verifies: FLOW-SEND-MESSAGE-R13
  it("asks Core about at most 8 at once, queuing the rest, and attaches them all in the order picked", async () => {
    const shell = fakeShell();
    const picked = Array.from({ length: 10 }, (_, i) => `D:/code/beta/f${i}.md`);
    shell.pickFiles.mockResolvedValue(picked);
    const answers: (() => void)[] = [];
    const core = fakeCore({
      "session.subscribe": () => ({ last_seq: -1 }),
      "attachment.inspect": (params: { path: string }) =>
        new Promise((r) => answers.push(() => r({ path: params.path, name: params.path.slice(-6), kind: "file", size_bytes: 1 }))),
    });
    const inspects = () => core.calls.filter((c) => c.method === "attachment.inspect").length;
    render(createElement(App, { client: core.client, shell }));
    await settle();
    await settle();
    fireEvent.click(screen.getByRole("button", { name: "Attach", exact: true } as never));
    fireEvent.click(screen.getByRole("menuitem", { name: "Add files…" }));
    await settle();
    expect(inspects()).toBe(8);
    // One answered, the next queued one goes.
    answers[0]?.();
    await settle();
    expect(inspects()).toBe(9);
    // Answered out of order, they still attach in the order picked.
    for (const answer of answers.slice(1).reverse()) answer();
    await settle();
    answers[9]?.();
    await settle();
    expect(inspects()).toBe(10);
    expect(Array.from(document.querySelectorAll(".attachment-name")).map((n) => n.textContent)).toEqual(picked.map((p) => p.slice(-6)));
  });
});
