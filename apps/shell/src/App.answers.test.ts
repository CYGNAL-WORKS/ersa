// @vitest-environment jsdom
// The window's wiring for answering a permission request (FLOW-APPROVE-PERMISSION, Sending an answer): an answer sent
// from the shelf is the session's, so the transcript's card shows it on its way once the session is made active.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, CoreEvent, Method } from "./core/client";
import type { Envelope, PermissionRequested } from "./protocol/agent-events.gen";
import type { CoreHelloResult, Session } from "./protocol/core-api.gen";
import { EVENT_VERSION } from "./session/transcript";

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const session = (id: string, created_at: string, tile: Session["tile"]): Session => ({
  id,
  workspace_id: "w-alpha",
  host_id: "local",
  backend: "claude-code",
  model: "opus",
  effort: "medium",
  permission_mode: "default",
  state: tile.state,
  created_at,
  tile,
});

const request: PermissionRequested = {
  type: "permission.requested",
  request_id: "r1",
  tool_use_id: "t1",
  tool_name: "Write",
  input: { file_path: "notes.txt", content: "probe" },
  suggestions: [],
  flags: { default_to_no: false, suppress_always_allow: false, requires_user_interaction: false, classifier_approvable: false },
};

const requested: Envelope = {
  v: EVENT_VERSION,
  session_id: "s1",
  seq: 0,
  ts: "2026-11-06T09:00:00Z",
  source: { backend: "claude-code", backend_version: "2.1.302", input: "live" },
  event: request,
};

/** A Core with two sessions: s1 waits on a permission request, and s2 is the newest, so it is active at launch. */
function fakeCore() {
  let onEvent: (event: CoreEvent) => void = () => {};
  const calls: { method: Method; params: unknown }[] = [];
  const answers: Partial<Record<Method, (params: never) => unknown>> = {
    "core.status": () => ({ sessions_running: 0, sessions_waiting: 1, uptime_s: 1, db_path: "x" }),
    "project.list": () => [{ id: "alpha", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true }],
    "session.list": () => [
      session("s1", "2026-11-06T09:00:00Z", { state: "waiting_user", pending: [{ kind: "permission", request_id: "r1" }] }),
      session("s2", "2026-11-06T10:00:00Z", { state: "idle", pending: [] }),
    ],
    "workspace.list": () => [{ id: "w-alpha", project_id: "alpha", kind: "root", path: "D:/code/alpha", vcs_ref: "main", state: "ready" }],
    "backend.list": () => [],
    "settings.get": () => ({}),
    "session.subscribe": () => ({ view: "v1", last_seq: 0 }),
    "session.unsubscribe": () => ({}),
  };
  const client: CoreClient = {
    attach: (listener) => {
      onEvent = listener;
      return Promise.resolve<Attachment>({ how: "attached", hello });
    },
    call: (method, params) => {
      calls.push({ method, params });
      // An answer stays on its way: Core never answers it here.
      if (method === "session.decide_permission") return new Promise(() => {}) as never;
      const answer = answers[method];
      return (answer ? Promise.resolve(answer(params as never)) : Promise.reject({ kind: "unavailable", message: `no answer for ${method}` })) as never;
    },
  };
  // Core sends s1's request to whoever subscribed.
  const send = () => act(() => onEvent({ kind: "notification", method: "session.event", params: { event: requested } } as CoreEvent));
  return { client, calls, send };
}

/** Lets every answer the fake gave settle, and the renders they cause. */
const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));

// The open tile measures where its shelf joins it; jsdom lays nothing out.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

afterEach(cleanup);

describe("answering from the shelf", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R18
  it("sends the one answer, and keeps the answers unavailable on the transcript's card once the session is made active", async () => {
    const core = fakeCore();
    render(createElement(App, { client: core.client }));
    await settle();
    await settle();

    fireEvent.click(screen.getByRole("button", { name: /^Request from / }));
    await settle();
    await core.send();
    const shelf = screen.getByRole("dialog", { name: /^Request from / });
    fireEvent.click(within(shelf).getByRole("button", { name: "Allow" }));
    expect(core.calls.filter((c) => c.method === "session.decide_permission").map((c) => c.params)).toEqual([
      { id: "s1", request_id: "r1", behavior: "allow" },
    ]);
    // On its way: neither answer can be chosen again.
    expect(within(shelf).getByRole("button", { name: "Allow" })).toHaveProperty("disabled", true);
    expect(within(shelf).getByRole("button", { name: "Deny" })).toHaveProperty("disabled", true);

    // Making s1 active closes its shelf; its transcript reads the same request, still on its way.
    fireEvent.click(screen.getByRole("button", { name: /Needs permission/ }));
    await settle();
    await core.send();
    const card = screen.getByRole("region", { name: "Permission: Write" });
    expect(within(card).getByRole("button", { name: "Allow" })).toHaveProperty("disabled", true);
    expect(within(card).getByRole("button", { name: "Deny" })).toHaveProperty("disabled", true);
    expect(core.calls.filter((c) => c.method === "session.decide_permission")).toHaveLength(1);
  });
});
