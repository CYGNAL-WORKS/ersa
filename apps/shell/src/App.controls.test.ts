// @vitest-environment jsdom
// The session controls in the window against a fake Core (FLOW-NEW-SESSION-R30, R11): a running session's model,
// effort and permission mode change through Core, a refused change says why on the status line, and a host whose
// backends can't be listed says that detection failed.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, CoreEvent, Method } from "./core/client";
import type { Envelope, Event } from "./protocol/agent-events.gen";
import type { BackendInfo, CoreHelloResult, Session, Workspace } from "./protocol/core-api.gen";
import { EVENT_VERSION } from "./session/transcript";

afterEach(cleanup);

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const workspaces: Workspace[] = [{ id: "w1", project_id: "alpha", kind: "isolated", path: "D:/ws/alpha-1", vcs_ref: "ersa/fix", state: "ready" }];

const levels = ["low", "high"];
const claudeCode: BackendInfo = {
  name: "claude-code",
  label: "Claude Code",
  version: "2.1.316",
  status: "available",
  tested: { min: "2.1.0", max: "2.1.400" },
  models: [
    { id: "default", label: "Default (recommended)", default: true, resolved: "claude-opus-5-5", efforts: levels },
    { id: "opus", label: "Opus 5.5", default: false, resolved: "claude-opus-5-5", efforts: levels },
    { id: "sonnet", label: "Sonnet 5.5", default: false, resolved: "claude-sonnet-5-5", efforts: levels },
  ],
  efforts: [
    { id: "low", label: "Low", default: false },
    { id: "high", label: "High", default: false },
  ],
  permission_modes: [
    { id: "default", label: "Ask", default: true },
    { id: "plan", label: "Plan only", default: false },
  ],
};

const session = (more: Partial<Session> = {}): Session => ({
  id: "s1",
  title: "Fix the build",
  workspace_id: "w1",
  host_id: "local",
  backend: "claude-code",
  model: "opus",
  effort: "low",
  permission_mode: "default",
  state: "idle",
  created_at: "2026-11-01T09:00:00Z",
  archived: false,
  pinned: false,
  unseen: false,
  tile: { state: "idle", pending: [] },
  ...more,
});

type Answers = Partial<Record<Method, (params: never) => unknown>>;

function fakeCore(listed: Session | Session[], more: Answers = {}) {
  let list = Array.isArray(listed) ? listed : [listed];
  let onEvent: (event: CoreEvent) => void = () => {};
  const calls: { method: Method; params: unknown }[] = [];
  const answers: Answers = {
    "core.status": () => ({ sessions_running: 1, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => [{ id: "alpha", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true }],
    "session.list": () => list,
    "workspace.list": () => workspaces,
    "backend.list": () => [claudeCode],
    "settings.get": () => ({}),
    "session.subscribe": () => ({ view: "v1", last_seq: 0 }),
    "session.unsubscribe": () => ({}),
    ...more,
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
  // Core now lists the session with the change, as `session.list` would after it.
  const becomes = (changed: Partial<Session>) => {
    list = list.map((s) => ({ ...s, ...changed }));
    return list[0] as Session;
  };
  // Ersa loses Core; attaching again waits, so Core stays away.
  const drop = () => act(() => onEvent({ kind: "lost", reason: "gone" }));
  // Delivers a session's next events, numbered on from its last, to whoever subscribed.
  const seqs = new Map<string, number>();
  const play = (sessionId: string, ...events: Event[]) =>
    events.forEach((e) => {
      const seq = seqs.get(sessionId) ?? 0;
      seqs.set(sessionId, seq + 1);
      const event: Envelope = {
        v: EVENT_VERSION,
        session_id: sessionId,
        seq,
        ts: "2026-11-15T12:00:00Z",
        source: { backend: "claude-code", backend_version: "2.1.302", input: "live" },
        event: e,
      };
      act(() => onEvent({ kind: "notification", method: "session.event", params: { event } } as CoreEvent));
    });
  return { client, calls, becomes, drop, play };
}

const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));

async function start(core: ReturnType<typeof fakeCore>) {
  render(createElement(App, { client: core.client }));
  await settle();
  await settle();
  await settle();
}

const controls = () => screen.getByRole("group", { name: "Session controls" });
const chip = (name: string) => within(controls()).getByRole("button", { name: new RegExp(`^${name}:`) });
const statusLine = () => document.querySelector(".composer-status-line");

/** A promise the test settles. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

const refusal = (message: string) => ({ kind: "rpc", error: { code: -32000, message, data: { kind: "invalid_params" } } });

describe("the session controls under a running session", () => {
  // verifies: FLOW-NEW-SESSION-R30
  it.each([
    ["Model", "Sonnet 5.5", "session.set_model", { model: "sonnet" }, { model: "claude-sonnet-5-5", model_request: "sonnet" }, "Model: Sonnet 5.5"],
    ["Effort", "High", "session.set_effort", { effort: "high" }, { effort: "high" }, "Effort: High"],
    ["Permission mode", "Plan only", "session.set_permission_mode", { permission_mode: "plan" }, { permission_mode: "plan" }, "Permission mode: Plan only"],
  ] as const)("change the %s through Core, reading the old value and keeping focus until Core answers", async (name, option, method, params, answer, reads) => {
    const pending = deferred<Session>();
    const core = fakeCore(session(), { [method]: () => pending.promise });
    await start(core);
    const before = chip(name).getAttribute("aria-label");
    act(() => chip(name).focus());
    fireEvent.click(chip(name));
    fireEvent.click(screen.getByRole("option", { name: new RegExp(option) }));
    await settle();
    expect(core.calls.find((c) => c.method === method)?.params).toEqual({ id: "s1", ...params });
    // On its way: the old value, focus on the chip, and the chip does not open.
    expect(chip(name).getAttribute("aria-label")).toBe(before);
    expect(chip(name).getAttribute("aria-busy")).toBe("true");
    expect(document.activeElement).toBe(chip(name));
    fireEvent.click(chip(name));
    expect(screen.queryByRole("listbox")).toBeNull();
    // Core answers with the session as it lists it: the new value, focus still on the chip.
    await act(async () => pending.resolve(core.becomes(answer)));
    await settle();
    expect(chip(name).getAttribute("aria-label")).toBe(reads);
    expect(chip(name).hasAttribute("aria-busy")).toBe(false);
    expect(document.activeElement).toBe(chip(name));
    expect(statusLine()).toBeNull();
  });

  // verifies: FLOW-NEW-SESSION-R30
  it("lists only the effort levels, with no Default row", async () => {
    await start(fakeCore(session()));
    fireEvent.click(chip("Effort"));
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["Low", "High"]);
  });

  // verifies: FLOW-NEW-SESSION-R30
  it("keeps a refused change's value and says why on the status line until the user opens a chip", async () => {
    const core = fakeCore(session(), {
      "session.set_effort": () => {
        throw refusal("session.set_effort: the model takes no effort high");
      },
    });
    await start(core);
    act(() => chip("Effort").focus());
    fireEvent.click(chip("Effort"));
    fireEvent.click(screen.getByRole("option", { name: /High/ }));
    await settle();
    expect(chip("Effort").getAttribute("aria-label")).toBe("Effort: Low");
    expect(document.activeElement).toBe(chip("Effort"));
    const line = screen.getByRole("alert");
    expect(line.textContent).toBe("Ersa can't change the effort. The model takes no effort high.");
    expect(line.querySelector('.glyph[data-tone="danger"]')).not.toBeNull();
    // Opening any chip clears it.
    fireEvent.click(chip("Model"));
    expect(statusLine()).toBeNull();
  });

  // verifies: FLOW-NEW-SESSION-R30
  it("holds each chip on its own, and keeps one change's refusal when another is accepted after it", async () => {
    const effort = deferred<Session>();
    const core = fakeCore(session(), {
      "session.set_effort": () => effort.promise,
      "session.set_model": () => {
        throw refusal("session.set_model: the backend refused the model");
      },
    });
    await start(core);
    fireEvent.click(chip("Effort"));
    fireEvent.click(screen.getByRole("option", { name: /High/ }));
    await settle();
    fireEvent.click(chip("Model"));
    fireEvent.click(screen.getByRole("option", { name: /Sonnet/ }));
    await settle();
    // The model's refusal is said, and the effort's change is still on its way.
    expect(statusLine()?.textContent).toBe("Ersa can't change the model. The backend refused the model.");
    expect(chip("Effort").getAttribute("aria-busy")).toBe("true");
    expect(chip("Model").hasAttribute("aria-busy")).toBe(false);
    // The effort's answer arrives: the refusal stays, since the user has neither sent nor opened a chip.
    await act(async () => effort.resolve(core.becomes({ effort: "high" })));
    await settle();
    expect(chip("Effort").getAttribute("aria-label")).toBe("Effort: High");
    expect(chip("Effort").hasAttribute("aria-busy")).toBe(false);
    expect(statusLine()?.textContent).toBe("Ersa can't change the model. The backend refused the model.");
  });

  // verifies: FLOW-NEW-SESSION-R30
  it("clears a refused change's line when the user sends", async () => {
    const core = fakeCore(session(), {
      "session.set_permission_mode": () => {
        throw refusal("session.set_permission_mode: the session is not live");
      },
      "session.send": () => ({ turn: 1 }),
    });
    await start(core);
    fireEvent.click(chip("Permission mode"));
    fireEvent.click(screen.getByRole("option", { name: /Plan only/ }));
    await settle();
    expect(statusLine()?.textContent).toBe("Ersa can't change the permission mode. The session is not live.");
    const words = screen.getByRole("textbox", { name: "Message" });
    fireEvent.change(words, { target: { value: "Run the tests" } });
    await act(async () => {
      fireEvent.keyDown(words, { key: "Enter" });
    });
    await settle();
    expect(statusLine()).toBeNull();
  });

  // verifies: FLOW-NEW-SESSION-R30
  it("opens none of its chips once the session has ended, keeping the chevron's place on model, effort and permission mode", async () => {
    await start(fakeCore(session({ state: "ended", tile: { state: "ended", pending: [] } })));
    for (const name of ["Model", "Effort", "Permission mode"]) {
      expect((chip(name) as HTMLButtonElement).disabled).toBe(true);
      expect(chip(name).querySelector('.chip-chevron[data-hidden="true"]')).not.toBeNull();
    }
    // The Worktree checkbox shows, unavailable, that the session runs in a worktree (issue 412).
    expect(screen.getByRole("checkbox", { name: "Worktree" })).toMatchObject({ checked: true, disabled: true });
  });

  // verifies: FLOW-NEW-SESSION-R30
  it("opens model, effort and permission mode while the session runs, and the other chips stay read-only", async () => {
    await start(fakeCore(session({ state: "running", tile: { state: "running", pending: [] } })));
    for (const name of ["Model", "Effort", "Permission mode"]) {
      expect((chip(name) as HTMLButtonElement).disabled).toBe(false);
      expect(chip(name).querySelector(".chip-chevron:not([data-hidden])")).not.toBeNull();
    }
    expect((chip("Project") as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByRole("checkbox", { name: "Worktree" })).toMatchObject({ checked: true, disabled: true });
  });
});

describe("a running session's controls across sessions and connections", () => {
  const tile = (title: string) => [...document.querySelectorAll<HTMLElement>(".session")].find((el) => el.textContent?.includes(title)) as HTMLElement;

  // verifies: FLOW-NEW-SESSION-R34
  it("keep a refused change's line for its session through a switch to another session and back", async () => {
    const other = session({ id: "s2", title: "Write the notes", created_at: "2026-10-01T09:00:00Z" });
    const core = fakeCore([session(), other], {
      "session.set_model": () => {
        throw refusal("session.set_model: the backend refused the model");
      },
    });
    await start(core);
    fireEvent.click(chip("Model"));
    fireEvent.click(screen.getByRole("option", { name: /Sonnet/ }));
    await settle();
    const said = "Ersa can't change the model. The backend refused the model.";
    expect(statusLine()?.textContent).toBe(said);
    // Another session has no refusal of its own.
    fireEvent.click(tile("Write the notes"));
    await settle();
    expect(statusLine()).toBeNull();
    // Back in the first, the line is said again, since the user neither sent nor opened a chip there.
    fireEvent.click(tile("Fix the build"));
    await settle();
    expect(statusLine()?.textContent).toBe(said);
  });

  // verifies: FLOW-NEW-SESSION-R35
  it("are unavailable while Ersa can't reach the sessions", async () => {
    const core = fakeCore(session());
    await start(core);
    for (const name of ["Model", "Effort", "Permission mode"]) expect((chip(name) as HTMLButtonElement).disabled).toBe(false);
    await core.drop();
    await settle();
    for (const name of ["Model", "Effort", "Permission mode"]) expect((chip(name) as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("a host whose backends can't be listed", () => {
  // verifies: FLOW-NEW-SESSION-R11
  it("says on the new-session screen that detection failed, with the reason, not that no backend is available", async () => {
    const core = fakeCore(session(), {
      "session.list": () => [],
      "backend.list": () => {
        throw refusal("backend.list: the host did not answer");
      },
    });
    await start(core);
    expect(statusLine()?.textContent).toBe("Ersa couldn't detect the backends on this host. the host did not answer");
    expect(document.body.textContent).not.toContain("No backend is available");
    expect(chip("Backend").getAttribute("aria-label")).toBe("Backend: Detection failed");
    expect(chip("Backend").querySelector('.glyph[data-tone="danger"]')).not.toBeNull();
  });
});

// LAY-SHELL, Context; docs/design/COMPOSER.md, The context ring (issue #422): the session controls' ring reads the
// nearest limit, the context or a usage window, and lists every reading; a tile's ring reads the context alone.
describe("the session controls' ring", () => {
  // Both sessions at 30 percent of their context; s1, the newer, shows at launch.
  const thirty = { state: "idle" as const, pending: [], context_tokens: 60_000, context_window: 200_000 };
  const sessions = [
    session({ tile: thirty, created_at: "2026-11-02T09:00:00Z" }),
    session({ id: "s2", title: "Write the docs", tile: thirty, created_at: "2026-11-01T09:00:00Z" }),
  ];
  const ring = () => within(controls()).getByRole("img");
  // The ring beside a tile's title in the rail.
  const tileRing = (title: string) => {
    const tile = [...document.querySelectorAll(".session-title")].find((e) => e.textContent === title)?.parentElement?.parentElement;
    return tile?.querySelector(".session-ring .context-ring") as HTMLElement;
  };
  const limits = (...windows: { label: string; used: number; resets_at?: string }[]): Event => ({
    type: "usage.updated",
    rate_limit: { status: "allowed", windows: windows.map((w, i) => ({ id: `w${i}`, ...w })) },
  });

  it("reads a window nearer its limit than the context, in danger, and names it", async () => {
    const core = fakeCore(sessions);
    await start(core);
    core.play("s1", limits({ label: "Weekly · all models", used: 0.2 }, { label: "5-hour limit", used: 0.84 }));
    expect(ring().getAttribute("aria-label")).toBe("5-hour limit: 84 percent used");
    expect(ring().getAttribute("data-band")).toBe("danger");
    expect(ring().textContent).toBe("84%");
    // The tile's ring for the same session reads the context alone.
    expect(tileRing("Fix the build").getAttribute("aria-label")).toBe("Context: 30 percent used");
  });

  it("reads the context when a window is at the same share", async () => {
    const core = fakeCore(sessions);
    await start(core);
    core.play("s1", limits({ label: "5-hour limit", used: 0.3 }));
    expect(ring().getAttribute("aria-label")).toBe("Context: 30 percent used");
    expect(ring().hasAttribute("data-band")).toBe(false);
  });

  it("lists every reading, the context first, then each window in the order reported", async () => {
    const core = fakeCore(sessions);
    await start(core);
    core.play("s1", limits({ label: "Weekly · all models", used: 0.62 }, { label: "5-hour limit", used: 0.84 }));
    const list = document.getElementById(ring().getAttribute("aria-describedby") ?? "");
    expect(list?.getAttribute("role")).toBe("tooltip");
    expect([...(list?.children ?? [])].map((line) => line.firstElementChild?.textContent)).toEqual([
      "Context: 30 percent used",
      "Weekly · all models: 62 percent used",
      "5-hour limit: 84 percent used",
    ]);
    // Keyboard focus reaches it, so the list can be read without a pointer.
    expect(ring().tabIndex).toBe(0);
  });

  it("reads the context alone on a session with no windows and on the new-session screen", async () => {
    const core = fakeCore(sessions);
    await start(core);
    core.play("s1", limits({ label: "5-hour limit", used: 0.84 }));
    expect(ring().getAttribute("aria-label")).toBe("5-hour limit: 84 percent used");
    fireEvent.click(screen.getByText("Write the docs"));
    await settle();
    expect(ring().getAttribute("aria-label")).toBe("Context: 30 percent used");
    fireEvent.click(screen.getByRole("button", { name: "New session in alpha" }));
    await settle();
    expect(ring().getAttribute("aria-label")).toBe("Context: none used yet");
  });
});
