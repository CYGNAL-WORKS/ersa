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
  return { client, calls, drop: () => act(() => onEvent({ kind: "lost", reason: "gone" })) };
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
