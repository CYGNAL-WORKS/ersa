// @vitest-environment jsdom
// The window's wiring for links in the agent's words (FLOW-READ-TRANSCRIPT-R03): a link the user chooses reaches the
// Shell's opener, which opens it in the system's default handler.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, CoreEvent, Method } from "./core/client";
import type { ShellControl } from "./core/shell";
import type { Envelope } from "./protocol/agent-events.gen";
import type { CoreHelloResult, Session } from "./protocol/core-api.gen";
import { EVENT_VERSION } from "./session/transcript";

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const session: Session = {
  id: "s1",
  workspace_id: "w-alpha",
  host_id: "local",
  backend: "claude-code",
  model: "opus",
  effort: "medium",
  permission_mode: "default",
  state: "idle",
  created_at: "2026-11-13T09:00:00Z",
  archived: false,
  pinned: false,
  unseen: false,
  tile: { state: "idle", pending: [] },
};

/** A Core with one session, s1, active at launch; `send` delivers s1's agent text to whoever subscribed. */
function fakeCore() {
  let onEvent: (event: CoreEvent) => void = () => {};
  const answers: Partial<Record<Method, (params: never) => unknown>> = {
    "core.status": () => ({ sessions_running: 0, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => [{ id: "alpha", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true }],
    "session.list": () => [session],
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
      const answer = answers[method];
      return (answer ? Promise.resolve(answer(params as never)) : Promise.reject({ kind: "unavailable", message: `no answer for ${method}` })) as never;
    },
  };
  const send = (text: string) => {
    const event: Envelope = {
      v: EVENT_VERSION,
      session_id: "s1",
      seq: 0,
      ts: "2026-11-13T09:00:00Z",
      source: { backend: "claude-code", backend_version: "2.1.302", input: "live" },
      event: { type: "assistant.text", block_id: "m1/0", text, final: true },
    };
    act(() => onEvent({ kind: "notification", method: "session.event", params: { event } } as CoreEvent));
  };
  return { client, send };
}

const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));

function fakeShell() {
  return {
    onQuitRequested: () => () => {},
    quitHeard: async () => {},
    show: async () => false,
    hide: async () => {},
    quit: async () => {},
    pickFolder: async () => null,
    pickFiles: async () => [] as string[],
    onDragDrop: () => () => {},
    openLink: vi.fn((_url: string) => {}),
  } satisfies ShellControl;
}

afterEach(cleanup);

describe("a link in the agent's words", () => {
  // verifies: FLOW-READ-TRANSCRIPT-R03
  it("reaches the Shell's opener when the user chooses it, and one with another scheme does not", async () => {
    const core = fakeCore();
    const shell = fakeShell();
    render(createElement(App, { client: core.client, shell }));
    await settle();
    await settle();
    core.send("Read [the guide](https://example.com/guide), write to [us](mailto:team@example.com), not [this](file:///C:/x.txt).");
    fireEvent.click(screen.getByRole("link", { name: "the guide" }));
    fireEvent.click(screen.getByRole("link", { name: "us" }));
    fireEvent.click(screen.getByText("this", { exact: false }));
    expect(shell.openLink.mock.calls).toEqual([["https://example.com/guide"], ["mailto:team@example.com"]]);
  });
});
