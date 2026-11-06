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

/** A Core that answers the reads App makes at launch, attaches once, and can drop the connection. */
function fakeCore() {
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
      return attaches++ === 0 ? Promise.resolve<Attachment>({ how: "attached", hello }) : new Promise(() => {});
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
  const gamma = screen.getAllByRole("listitem").find((li) => li.querySelector(".project-name")?.textContent === "gamma");
  fireEvent.click(within(gamma!).getByRole("button", { name: "New session" }));
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
