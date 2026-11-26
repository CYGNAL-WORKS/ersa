// @vitest-environment jsdom
// The sign-in note under a session (FLOW-SIGN-IN-R17, R18), against a fake Core and a scripted event stream: the note
// shows when a live session's message fails as not signed in, for that session's backend on its host, and goes with a
// later message of the person's or a sign-in for that backend that ended signed in; the box, send and status line
// stay as they are, and no message is held back. No real sign-in is started; the address is a placeholder.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, CoreEvent, Method } from "./core/client";
import type { ShellControl } from "./core/shell";
import type { BackendInfo, BackendSignInEndedNotification, CoreHelloResult, Session } from "./protocol/core-api.gen";
import type { Envelope } from "./protocol/agent-events.gen";
import { EVENT_VERSION } from "./session/transcript";

afterEach(cleanup);

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const PAGE = "https://sign-in.example/start";

// The session's backend lists available, as a lapsed sign-in may: the note follows the session's own failure.
const backend = (name: string, label: string, more: Partial<BackendInfo> = {}): BackendInfo => ({
  name,
  label,
  version: "2.1.316",
  status: "available",
  tested: { min: "2.1.0", max: "2.1.400" },
  models: [{ id: "opus", label: "Opus", default: true, efforts: ["high"] }],
  efforts: [{ id: "high", label: "High", default: true }],
  permission_modes: [{ id: "default", label: "Manual", default: true }],
  ...more,
});

/** The one session, live (its backend idle after the failed turn) unless `tile` says otherwise. */
const session = (more: Partial<Session> = {}): Session => ({
  id: "s1",
  workspace_id: "w1",
  host_id: "local",
  backend: "claude-code",
  model: "opus",
  effort: "high",
  permission_mode: "default",
  state: "idle",
  created_at: "2026-11-26T09:00:00Z",
  archived: false,
  pinned: false,
  unseen: false,
  tile: { state: "idle", pending: [] },
  ...more,
});

type Answer = (params: never) => unknown;

function fakeCore({ listed = session(), backends = [backend("claude-code", "Claude Code (work)")] } = {}) {
  const calls: { method: Method; params: unknown }[] = [];
  let onEvent: (event: CoreEvent) => void = () => {};
  const core = { listed, backends, send: (() => ({ turn: 2 })) as Answer };
  const answers: Partial<Record<Method, Answer>> = {
    "core.status": () => ({ sessions_running: 1, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => [{ id: "alpha", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "none", trusted: true }],
    "session.list": () => [core.listed],
    "workspace.list": () => [{ id: "w1", project_id: "alpha", kind: "root", path: "D:/code/alpha", state: "ready" }],
    "backend.list": () => core.backends,
    "backend.sign_in": () => ({ url: PAGE }),
    "backend.cancel_sign_in": () => ({}),
    "session.subscribe": () => ({ last_seq: -1 }),
    "session.send": (params) => core.send(params),
    "settings.get": () => ({}),
  };
  const client: CoreClient = {
    attach: (listener) => {
      onEvent = listener;
      return Promise.resolve<Attachment>({ how: "attached", hello });
    },
    call: (method, params) => {
      calls.push({ method, params });
      const answer = answers[method];
      if (!answer) return Promise.reject({ kind: "unavailable", message: `no answer for ${method}` }) as never;
      return new Promise((resolve) => resolve(answer(params as never))) as never;
    },
  };
  const emit = (event: CoreEvent) => act(async () => onEvent(event));
  /** An event on s1, from the live stream. */
  const on = (seq: number, event: Envelope["event"]) =>
    emit({
      kind: "notification",
      method: "session.event",
      params: {
        event: { v: EVENT_VERSION, session_id: "s1", seq, ts: "2026-11-26T09:00:00Z", source: { backend: "claude-code", backend_version: "2.1.316", input: "live" }, event },
      },
    } as CoreEvent);
  const ended = (ending: Omit<BackendSignInEndedNotification, "host_id" | "name">, who: { host_id?: string; name?: string } = {}) =>
    emit({ kind: "notification", method: "backend.sign_in_ended", params: { host_id: "local", name: "claude-code", ...ending, ...who } });
  return Object.assign(core, { client, calls, emit, on, ended, called: (method: Method) => calls.filter((c) => c.method === method) });
}

const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));
const settled = async () => {
  for (let i = 0; i < 6; i += 1) await settle();
};

const openLink = vi.fn<(url: string) => void>();
const shell = { onWindowBack: () => () => {}, onQuitRequested: () => () => {}, onDragDrop: () => () => {}, openLink } as unknown as ShellControl;
afterEach(() => openLink.mockReset());

async function open(core: ReturnType<typeof fakeCore>) {
  render(createElement(App, { client: core.client, shell }));
  await settled();
}

const failure = { type: "error", code: "authentication_failed", message: "Not logged in · Please run /login", recoverable: true, action: "log_in" } as const;
/** The session's first message, its turn failing as not signed in, and the backend idle again (as recorded). */
const failedTurn = async (core: ReturnType<typeof fakeCore>, from = 0) => {
  await core.on(from, { type: "user.message", text: "Fix the build", origin: "human" });
  await core.on(from + 1, failure);
  await settled();
};

const note = () => document.querySelector<HTMLElement>(".sign-in-note");
const composer = () => screen.getByRole("form", { name: "Composer" });
const message = () => within(composer()).getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
const send = async (text: string) => {
  fireEvent.change(message(), { target: { value: text } });
  await act(async () => {
    fireEvent.keyDown(message(), { key: "Enter" });
  });
  await settled();
};

describe("the sign-in note under a session", () => {
  // verifies: FLOW-SIGN-IN-R17
  it("shows above the status line once a live session's message fails as not signed in, for its backend on its host", async () => {
    const core = fakeCore();
    await open(core);
    expect(note()).toBeNull();
    await failedTurn(core);
    // The session's line, by the session's backend's label; Sign in offered.
    expect(note()?.textContent).toContain("Claude Code (work) isn't signed in on this host. Sign in, then send your message again.");
    const signIn = within(note()!).getByRole("button", { name: "Sign in" });
    // Above the status line and the box, in the composer's column.
    const column = note()!.closest(".composer")!;
    const order = [...column.querySelectorAll(".sign-in-note, .composer-status, .composer-box")].map((el) => el.className.split(" ")[0]);
    expect(order).toEqual(["sign-in-note", "composer-status", "composer-box"]);
    // Sign in starts that backend's sign-in on the session's host, and opens the page (R03, R04), behaving as there.
    await act(async () => fireEvent.click(signIn));
    await settled();
    expect(core.called("backend.sign_in").map((c) => c.params)).toEqual([{ host_id: "local", name: "claude-code" }]);
    expect(openLink).toHaveBeenCalledWith(PAGE);
  });

  // verifies: FLOW-SIGN-IN-R17
  it("follows the host's listing while the sign-in is under way, and keeps a failure's words, as the note does elsewhere", async () => {
    const core = fakeCore();
    await open(core);
    await failedTurn(core);
    await act(async () => fireEvent.click(within(note()!).getByRole("button", { name: "Sign in" })));
    // The listing read on Core's answer reports it under way (R06).
    core.backends = [backend("claude-code", "Claude Code (work)", { signing_in: { url: PAGE } })];
    await settled();
    // Read again on its end (R09); still listed available, which on the new-session screen means no note at all.
    core.backends = [backend("claude-code", "Claude Code (work)")];
    await core.ended({ outcome: "timed_out" });
    await settled();
    // The words a failure adds stay while the note does (R14, R15).
    expect(note()?.textContent).toContain("Claude Code (work)'s sign-in took too long and was stopped.");
  });

  // verifies: FLOW-SIGN-IN-R17
  it("shows for a session that is not live no note", async () => {
    const core = fakeCore({ listed: session({ state: "ended", tile: { state: "ended", pending: [] } }) });
    await open(core);
    await failedTurn(core);
    // The failure's line is in the transcript, and the composer has no note.
    expect(screen.getByRole("region", { name: "Transcript" }).textContent).toContain("Claude Code (work) wasn't signed in.");
    expect(note()).toBeNull();
  });

  // verifies: FLOW-SIGN-IN-R17
  it("shows for an archived session, which has no composer, no note", async () => {
    const core = fakeCore();
    await open(core);
    await failedTurn(core);
    expect(note()).not.toBeNull();
    // Archived elsewhere while shown (which stops it): a new session's tile makes App list the sessions again.
    core.listed = session({ archived: true, state: "ended", tile: { state: "ended", pending: [] } });
    await core.emit({ kind: "notification", method: "session.tile", params: { session_id: "s9", tile: { state: "idle", pending: [] }, created_at: "2026-11-27T09:00:00Z" } } as CoreEvent);
    await settled();
    expect(screen.getByText("This session is archived. Resume it to send a message.")).toBeTruthy();
    expect(screen.queryByRole("form", { name: "Composer" })).toBeNull();
    expect(note()).toBeNull();
  });

  // verifies: FLOW-SIGN-IN-R17
  it("goes with a later message of the person's, from this window or another", async () => {
    const core = fakeCore();
    await open(core);
    await failedTurn(core);
    expect(note()).not.toBeNull();
    // From this window: it goes once Core accepts the message, before its echo arrives.
    await send("Try again");
    expect(note()).toBeNull();
    // It fails again: the note is back.
    await core.on(2, { type: "turn.started", turn: 2, trigger: "user" });
    await failedTurn(core, 3);
    expect(note()).not.toBeNull();
    // From another window: its message arrives on the stream.
    await core.on(5, { type: "user.message", text: "From the other window", origin: "human" });
    await settled();
    expect(note()).toBeNull();
  });

  // verifies: FLOW-SIGN-IN-R17
  it("goes once a sign-in for that backend on that host ends signed in, and not for another's or another ending", async () => {
    const core = fakeCore();
    await open(core);
    await failedTurn(core);
    // Another backend, another host, or an ending other than signed in: the note stays.
    await core.ended({ outcome: "signed_in" }, { name: "copilot" });
    await core.ended({ outcome: "signed_in" }, { host_id: "devbox-2" });
    await core.ended({ outcome: "failed" });
    await core.ended({ outcome: "canceled" });
    await settled();
    expect(note()).not.toBeNull();
    await core.ended({ outcome: "signed_in" });
    await settled();
    expect(note()).toBeNull();
    // A failure after that sign-in shows the note again.
    await core.on(2, failure);
    await settled();
    expect(note()).not.toBeNull();
  });
});

describe("the box, send and status line under a session with the sign-in note", () => {
  // verifies: FLOW-SIGN-IN-R18
  it("leaves them as they are without it, and holds back no message", async () => {
    const core = fakeCore();
    // The backend even lists signed out: still nothing is held back.
    core.backends = [backend("claude-code", "Claude Code (work)", { status: "signed_out" })];
    // Core refuses the first send, and takes the second.
    let refuse = true;
    core.send = () => {
      if (!refuse) return { turn: 2 };
      refuse = false;
      throw { kind: "rpc", error: { code: -32000, message: "The session is busy.", data: { kind: "session_busy" } } };
    };
    await open(core);
    await failedTurn(core);
    expect(note()).not.toBeNull();
    // The box is available, and takes words.
    expect(composer().hasAttribute("data-unavailable")).toBe(false);
    expect(message().disabled).toBe(false);
    fireEvent.change(message(), { target: { value: "Try again" } });
    const sendButton = within(composer()).getByRole("button", { name: "Send" });
    expect(sendButton.getAttribute("aria-disabled")).toBeNull();
    // Send goes to Core at once; refused, the status line says why beneath the note, which a message Core did not
    // take leaves in place.
    await act(async () => fireEvent.click(sendButton));
    await settled();
    expect(core.called("session.send").map((c) => c.params)).toEqual([{ id: "s1", text: "Try again" }]);
    const line = composer().closest(".composer")!.querySelector(".composer-status-line");
    expect(line?.textContent).toContain("The session is busy.");
    expect(note()).not.toBeNull();
    expect(note()!.compareDocumentPosition(line!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // Sent again and taken: the note goes with it.
    await send("Try again");
    expect(core.called("session.send")).toHaveLength(2);
    expect(note()).toBeNull();
  });
});
