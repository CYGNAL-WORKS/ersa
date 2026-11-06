// @vitest-environment jsdom
// The window's wiring, rendered in a DOM against a fake Core: what App hands the new-session screen.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, CoreEvent, Method } from "./core/client";
import type { CoreHelloResult, Project, Session, Workspace } from "./protocol/core-api.gen";

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

/** A Core that answers the reads App makes at launch, attaches once (or each time, with `reattach`), and can drop
 * the connection. */
function fakeCore({ reattach = false } = {}) {
  let onEvent: (event: CoreEvent) => void = () => {};
  let attaches = 0;
  const answers: Partial<Record<Method, (params: never) => unknown>> = {
    "core.status": () => ({ sessions_running: 0, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => projects,
    "session.list": () => sessions,
    "workspace.list": (params: { project_id: string }) => [root(params.project_id)],
    "backend.list": () => [],
    "settings.get": () => ({}),
  };
  const client: CoreClient = {
    attach: (listener) => {
      onEvent = listener;
      // Attaching again after a loss never answers: Core stays away.
      return attaches++ === 0 || reattach ? Promise.resolve<Attachment>({ how: "attached", hello }) : new Promise(() => {});
    },
    call: (method, params) => {
      const answer = answers[method];
      return (answer ? Promise.resolve(answer(params as never)) : Promise.reject({ kind: "unavailable", message: `no answer for ${method}` })) as never;
    },
  };
  return { client, drop: () => act(() => onEvent({ kind: "lost", reason: "gone" })) };
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
    const core = fakeCore({ reattach: true });
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
