// @vitest-environment jsdom
// The window's wiring for Fork (FLOW-FORK-SESSION): the tile's Fork, when it is unavailable, the fork becoming the
// active session, and a refusal said beneath the original's tile.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";
import type { Attachment, BridgeError, CoreClient, Method } from "./core/client";
import type { CoreHelloResult, Session, Workspace } from "./protocol/core-api.gen";

afterEach(cleanup);

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const workspaces: Workspace[] = [{ id: "w-root", project_id: "alpha", kind: "root", path: "D:/code/alpha", vcs_ref: "", state: "ready" }];

const session = (id: string, title: string, created_at: string, more: Partial<Session> = {}): Session => ({
  id,
  title,
  workspace_id: "w-root",
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  state: "idle",
  created_at,
  last_activity_at: created_at,
  archived: false,
  pinned: false,
  tile: { state: "idle", pending: [] },
  ...more,
});

// Newest first in the rail: s3, s2, s1. The newest is shown first.
const listed = [
  session("s1", "Fix the build", "2026-11-01T09:00:00Z"),
  session("s2", "Write the notes", "2026-11-02T09:00:00Z", { state: "running", tile: { state: "running", pending: [] } }),
  session("s3", "Tidy the docs", "2026-11-03T09:00:00Z"),
];

const forked = session("f1", "Fix the build (fork)", "2026-11-01T09:00:00Z", { last_activity_at: undefined, tile: { state: "starting", pending: [] } });

function fakeCore(fork: (params: { id: string }) => unknown) {
  let list = listed;
  const calls: { method: Method; params: unknown }[] = [];
  const answers: Partial<Record<Method, (params: never) => unknown>> = {
    "core.status": () => ({ sessions_running: 1, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => [{ id: "alpha", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true }],
    "session.list": () => list,
    "workspace.list": () => workspaces,
    "backend.list": () => [],
    "settings.get": () => ({}),
    "session.subscribe": () => ({ view: "v1", last_seq: 0 }),
    "session.unsubscribe": () => ({}),
    "session.fork": (params: { id: string }) => {
      const answer = fork(params);
      list = [...list, answer as Session];
      return answer;
    },
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
const tile = (title: string) => [...rail().querySelectorAll<HTMLElement>(".session")].find((el) => el.textContent?.includes(title));
const forkItem = (title: string) => {
  fireEvent.click(within(rail()).getByRole("button", { name: `More for ${title}` }));
  return screen.getByRole("menuitem", { name: /^Fork/ });
};

async function open(fork: (params: { id: string }) => unknown = () => forked) {
  const core = fakeCore(fork);
  render(createElement(App, { client: core.client }));
  await settle();
  await settle();
  return core;
}

describe("Fork on a tile", () => {
  // verifies: FLOW-FORK-SESSION-R04
  it("starts the fork of the session and makes the fork the active session, its tile in the rail", async () => {
    const core = await open();
    // The newest session is shown at first, not the one forked.
    expect(tile("Tidy the docs")?.getAttribute("aria-current")).toBe("true");
    fireEvent.click(forkItem("Fix the build"));
    await settle();
    await settle();
    expect(core.calls.filter((c) => c.method === "session.fork").map((c) => c.params)).toEqual([{ id: "s1" }]);
    expect(tile("Fix the build (fork)")?.getAttribute("aria-current")).toBe("true");
    expect(tile("Fix the build")?.getAttribute("aria-current")).toBeNull();
    // The window follows the fork, so its transcript shows.
    expect(core.calls.some((c) => c.method === "session.subscribe" && (c.params as { id: string }).id === "f1")).toBe(true);
  });

  // verifies: FLOW-FORK-SESSION-R03
  it("is unavailable with its reason while a reply runs", async () => {
    const core = await open();
    const running = forkItem("Write the notes");
    expect(running.getAttribute("aria-disabled")).toBe("true");
    expect(running.textContent).toContain("Available once the reply finishes");
    fireEvent.click(running);
    await settle();
    expect(core.calls.some((c) => c.method === "session.fork")).toBe(false);
  });

  // verifies: FLOW-FORK-SESSION-R03
  it("says why beneath the original's tile when Core refuses, and the original stays active-or-not as it was", async () => {
    const refusal: BridgeError = {
      kind: "rpc",
      error: {
        code: -32000,
        message: "session.fork: the last turn in session s1 was interrupted; it can be forked once a reply finishes",
        data: { kind: "invalid_state" },
      },
    } as never;
    await open(() => {
      throw refusal;
    });
    fireEvent.click(forkItem("Fix the build"));
    await settle();
    await settle();
    const note = within(rail()).getByRole("alert");
    expect(note.textContent).toContain("Ersa can't fork this session.");
    expect(note.textContent).toContain("the last turn in session s1 was interrupted; it can be forked once a reply finishes");
    expect(tile("Tidy the docs")?.getAttribute("aria-current")).toBe("true");
  });
});
