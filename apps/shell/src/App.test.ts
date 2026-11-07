// @vitest-environment jsdom
// The window's wiring, rendered in a DOM against a fake Core: what App hands the new-session screen.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, CoreEvent, Method } from "./core/client";
import type { ShellControl } from "./core/shell";
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
  const answers: Answers = {
    "core.status": () => ({ sessions_running: 0, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => projects,
    "session.list": () => sessions,
    "workspace.list": (params: { project_id: string }) => [root(params.project_id)],
    "backend.list": () => [],
    "settings.get": () => ({}),
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
    return [...tree.querySelectorAll(".rail-project")].map((group) => [
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
    if (params.host_id !== "local") throw new Error("wrong host");
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
      { host_id: "local", path: "D:/code/beta/notes.md", workspace_id: "w-beta" },
      { host_id: "local", path: "C:/pics/shot.png", workspace_id: "w-beta" },
      { host_id: "local", path: "D:/code/beta/src", workspace_id: "w-beta" },
    ]);
    // R14: the note follows what Core says for the session's workspace; an image goes inline.
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

  // verifies: FLOW-SEND-MESSAGE-R14
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
      { host_id: "local", path: "D:/code/alpha/notes.md", workspace_id: "w-alpha" },
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
    expect(Array.from(document.querySelectorAll(".composer-attachment-name")).map((n) => n.textContent)).toEqual(picked.map((p) => p.slice(-6)));
  });
});
