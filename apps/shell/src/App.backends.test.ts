// @vitest-environment jsdom
// The new-session screen reads the host's backends again (FLOW-NEW-SESSION-R50 to R52, R11), against a fake Core: each
// time the screen is shown and each time the window is shown or takes the focus, with nothing said while it reads, a
// backend found then chosen as on a screen just opened, and the words for a backend Ersa can't find.

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, CoreEvent, Method } from "./core/client";
import type { ShellControl } from "./core/shell";
import type { BackendInfo, CoreHelloResult } from "./protocol/core-api.gen";

afterEach(cleanup);

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const claudeCode = (status: BackendInfo["status"] = "available"): BackendInfo => ({
  name: "claude-code",
  label: "Claude Code",
  version: status === "available" ? "2.1.316" : undefined,
  status,
  tested: { min: "2.1.0", max: "2.1.400" },
  models: [
    { id: "default", label: "Default (recommended)", default: true, resolved: "claude-opus-5-5", efforts: ["low", "high"] },
    { id: "opus", label: "Opus 5.5", default: false, resolved: "claude-opus-5-5", efforts: ["low", "high"] },
    { id: "sonnet", label: "Sonnet 5.5", default: false, resolved: "claude-sonnet-5-5", efforts: ["low", "high"] },
  ],
  efforts: [
    { id: "low", label: "Low", default: false },
    { id: "high", label: "High", default: true },
  ],
  permission_modes: [
    { id: "plan", label: "Plan only", default: false },
    { id: "default", label: "Manual", default: true },
  ],
});
const copilot = (status: BackendInfo["status"]): BackendInfo => ({ ...claudeCode(status), name: "copilot", label: "GitHub Copilot" });

type Answers = Partial<Record<Method, (params: never) => unknown>>;

/** What `backend.list` answers: a listing, or a function called for each reading, to hold or refuse it. */
type Listing = BackendInfo[] | (() => Promise<BackendInfo[]>);

/** A fake Core whose `backend.list` answers with `listing` until the test sets `core.listing` to another. */
function fakeCore(listing: Listing) {
  const calls: Method[] = [];
  const core = { listing, attached: 0 };
  let onEvent: (event: CoreEvent) => void = () => {};
  const answers: Answers = {
    "core.status": () => ({ sessions_running: 0, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => [{ id: "alpha", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "none", trusted: true }],
    "session.list": () => [],
    "workspace.list": () => [{ id: "w1", project_id: "alpha", kind: "root", path: "D:/code/alpha", state: "ready" }],
    "backend.list": () => (typeof core.listing === "function" ? core.listing() : core.listing),
    "settings.get": () => ({}),
    "session.subscribe": () => ({ view: "v1", last_seq: 0 }),
    "session.unsubscribe": () => ({}),
  };
  const client: CoreClient = {
    // Each attach answers at once: after a loss the window attaches again by itself.
    attach: (listener) => {
      onEvent = listener;
      core.attached += 1;
      return Promise.resolve<Attachment>({ how: "attached", hello });
    },
    call: (method, params) => {
      calls.push(method);
      const answer = answers[method];
      if (!answer) return Promise.reject({ kind: "unavailable", message: `no answer for ${method}` }) as never;
      return new Promise((resolve) => resolve(answer(params as never))) as never;
    },
  };
  // Ersa loses Core, as when its connection drops.
  const drop = () => act(() => onEvent({ kind: "lost", reason: "gone" }));
  return Object.assign(core, { client, drop, readings: () => calls.filter((m) => m === "backend.list").length });
}

const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));

// The Shell's controls the screen uses: the window coming back, which the test raises (`windowBack`).
let back = new Set<() => void>();
const shell = {
  onWindowBack: (handler: () => void) => {
    back.add(handler);
    return () => back.delete(handler);
  },
  onQuitRequested: () => () => {},
  onDragDrop: () => () => {},
} as unknown as ShellControl;
afterEach(() => {
  back = new Set();
});

async function start(core: ReturnType<typeof fakeCore>) {
  render(createElement(App, { client: core.client, shell }));
  for (let i = 0; i < 4; i += 1) await settle();
}

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

const rail = () => screen.getByRole("navigation", { name: "Rail" });
const controls = () => screen.getByRole("group", { name: "Session controls" });
const chip = (name: string) => within(controls()).getByRole("button", { name: new RegExp(`^${name}:`) });
const statusLine = () => document.querySelector(".composer-status-line");
const words = () => screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
const newSession = () => fireEvent.click(within(rail()).getByRole("button", { name: "New session" }));

/** The Shell saying the window came back: shown from the tray, or taking the focus. */
const windowBack = async () => {
  await act(async () => {
    for (const handler of [...back]) handler();
  });
  await settle();
};

describe("the new-session screen reading the host's backends again", () => {
  // verifies: FLOW-NEW-SESSION-R50
  it("reads them again each time the screen is shown and each time the window is shown or takes the focus", async () => {
    const core = fakeCore([claudeCode()]);
    await start(core);
    expect(screen.getByRole("region", { name: "New session" })).toBeTruthy();
    const first = core.readings();
    expect(first).toBeGreaterThanOrEqual(1);
    // The screen shown again.
    newSession();
    await settle();
    expect(core.readings()).toBe(first + 1);
    // The window shown again or taking the focus.
    await windowBack();
    expect(core.readings()).toBe(first + 2);
    await windowBack();
    expect(core.readings()).toBe(first + 3);
  });

  // How the Shell fails a request still waiting when Core's connection closes: unanswered, not refused by Core.
  const closed = { kind: "unavailable", message: "connection closed", sent: true };
  /** Waits until the window has attached again and says nothing of the lost connection. */
  const reattached = async (core: ReturnType<typeof fakeCore>) => {
    await waitFor(() => expect(core.attached).toBe(2), { timeout: 5_000 });
    await waitFor(() => expect(screen.queryByText(/reach the sessions|Reconnecting|trying again/i)).toBeNull(), { timeout: 5_000 });
    await settle();
  };

  // verifies: FLOW-NEW-SESSION-R50
  it.each([
    ["after the loss reaches the window", "after"],
    ["before the loss reaches the window", "before"],
  ] as const)("shows nothing on the next connection of a reading the lost connection failed, %s", async (_, order) => {
    const core = fakeCore([claudeCode()]);
    await start(core);
    const out = deferred<BackendInfo[]>();
    core.listing = () => out.promise;
    await windowBack();
    // Every state the page is drawn in from here on, so a failure shown for a single frame is caught.
    const drawn: string[] = [];
    const watch = new MutationObserver(() => drawn.push(document.body.textContent ?? ""));
    watch.observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true });
    // The Shell fails what waits on the connection and reports the loss by separate paths, in either order.
    const fail = () => act(async () => out.reject(closed));
    if (order === "before") await fail();
    await core.drop();
    // The next connection's reading is held, so whatever shows meanwhile is the lost reading's doing.
    const next = deferred<BackendInfo[]>();
    core.listing = () => next.promise;
    if (order === "after") await fail();
    await reattached(core);
    watch.disconnect();
    // Once attached again, no frame says detection failed, nor detects what is held. (Before the window learns of
    // the loss, a failure that lands first is the connection it still has failing, which R11 says.)
    const away = (text: string) => /reach the sessions|Reconnecting|trying again/i.test(text);
    const lost = drawn.findIndex(away);
    const sinceBack = drawn.slice(lost + drawn.slice(lost).findIndex((text) => !away(text)));
    expect(lost).toBeGreaterThanOrEqual(0);
    expect(sinceBack.filter((text) => text.includes("couldn't detect the backends") || text.includes("Detecting"))).toEqual([]);
    expect(document.body.textContent).not.toContain("couldn't detect the backends");
    expect(document.body.textContent).not.toContain("Detecting");
    expect(chip("Backend").getAttribute("aria-label")).toBe("Backend: Claude Code");
    expect(words().disabled).toBe(false);
  });

  // verifies: FLOW-NEW-SESSION-R23
  it.each([
    ["before the loss", "before"],
    ["only once the window has attached again", "reattached"],
  ] as const)("detects the backends again on the next connection when the lost one failed the first reading %s", async (_, order) => {
    const first = deferred<BackendInfo[]>();
    const core = fakeCore(() => first.promise);
    await start(core);
    const fail = () => act(async () => first.reject(closed));
    if (order === "before") await fail();
    await core.drop();
    const next = deferred<BackendInfo[]>();
    core.listing = () => next.promise;
    await reattached(core);
    if (order === "reattached") await fail();
    // No listing is held, so the new connection's first reading is detection, not a failure Ersa can't vouch for.
    expect(statusLine()?.textContent).toBe("Detecting the backends on this host…");
    expect(document.body.textContent).not.toContain("couldn't detect the backends");
    await act(async () => next.resolve([claudeCode()]));
    await settle();
    expect(chip("Backend").getAttribute("aria-label")).toBe("Backend: Claude Code");
  });

  // verifies: FLOW-NEW-SESSION-R11
  it("keeps what the new connection read over a late answer from the lost one", async () => {
    const core = fakeCore([claudeCode()]);
    await start(core);
    const out = deferred<BackendInfo[]>();
    core.listing = () => out.promise;
    await windowBack();
    await core.drop();
    // The new connection reads and Core refuses; then the lost connection's reading answers after all.
    core.listing = () => Promise.reject(refusal("backend.list: the host did not answer"));
    await waitFor(() => expect(core.attached).toBe(2), { timeout: 5_000 });
    await waitFor(() => expect(statusLine()?.textContent).toContain("couldn't detect the backends"), { timeout: 5_000 });
    await act(async () => out.resolve([claudeCode()]));
    await settle();
    expect(statusLine()?.textContent).toBe("Ersa couldn't detect the backends on this host. the host did not answer");
    expect(chip("Backend").getAttribute("aria-label")).toBe("Backend: Detection failed");
  });

  // verifies: FLOW-NEW-SESSION-R50
  it("reads them again behind the sessions page only once the screen shows again", async () => {
    const core = fakeCore([claudeCode()]);
    await start(core);
    fireEvent.click(within(rail()).getByRole("button", { name: "Arrange sessions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "View all" }));
    await settle();
    expect(screen.getByRole("heading", { name: "Sessions" })).toBeTruthy();
    const before = core.readings();
    await windowBack();
    expect(core.readings()).toBe(before);
    newSession();
    await settle();
    expect(screen.queryByRole("heading", { name: "Sessions" })).toBeNull();
    expect(core.readings()).toBe(before + 1);
  });

  // verifies: FLOW-NEW-SESSION-R50
  it("goes on showing the listing it holds, the composer as available as it was, until the answer arrives", async () => {
    const core = fakeCore([claudeCode()]);
    await start(core);
    fireEvent.change(words(), { target: { value: "Fix the build" } });
    const later = deferred<BackendInfo[]>();
    core.listing = () => later.promise;
    const before = core.readings();
    await windowBack();
    expect(core.readings()).toBe(before + 1);
    // Reading: nothing of it shows.
    expect(statusLine()).toBeNull();
    expect(document.body.textContent).not.toContain("Detecting");
    expect(chip("Backend").getAttribute("aria-label")).toBe("Backend: Claude Code");
    expect(words().disabled).toBe(false);
    // The answer changes the screen: no backend can run now.
    await act(async () => later.resolve([claudeCode("missing")]));
    await settle();
    const missingLine = statusLine()?.textContent;
    expect(missingLine).toContain("can't find Claude Code");
    // Reading again with no backend that can run: the line it holds stays, the composer as unavailable as it was.
    const held = deferred<BackendInfo[]>();
    core.listing = () => held.promise;
    await windowBack();
    expect(core.readings()).toBe(before + 2);
    expect(statusLine()?.textContent).toBe(missingLine);
    expect(document.body.textContent).not.toContain("Detecting");
    expect(chip("Backend").getAttribute("aria-label")).toBe("Backend: No backend");
    expect(words().disabled).toBe(true);
    expect(words().value).toBe("Fix the build");
  });

  // verifies: FLOW-NEW-SESSION-R11
  it("says that detection failed when a reading fails while a listing is held", async () => {
    const core = fakeCore([claudeCode()]);
    await start(core);
    expect(chip("Backend").getAttribute("aria-label")).toBe("Backend: Claude Code");
    core.listing = () => Promise.reject(refusal("backend.list: the host did not answer"));
    await windowBack();
    expect(statusLine()?.textContent).toBe("Ersa couldn't detect the backends on this host. the host did not answer");
    expect(chip("Backend").getAttribute("aria-label")).toBe("Backend: Detection failed");
    expect(words().disabled).toBe(true);
  });

  // verifies: FLOW-NEW-SESSION-R51
  it("chooses a backend found by a later reading, with its model, effort and permission mode, keeping the words", async () => {
    const core = fakeCore([claudeCode("missing")]);
    await start(core);
    expect(chip("Backend").getAttribute("aria-label")).toBe("Backend: No backend");
    // The words can't be typed while the box is unavailable, so they are set as the composer would hold them.
    fireEvent.change(words(), { target: { value: "Fix the build" } });
    core.listing = [claudeCode()];
    await windowBack();
    expect(chip("Backend").getAttribute("aria-label")).toBe("Backend: Claude Code");
    // As R02 starts a screen just opened: the backend's default model, effort and permission mode.
    expect(chip("Model").getAttribute("aria-label")).toBe("Model: Opus 5.5");
    expect(chip("Effort").getAttribute("aria-label")).toBe("Effort: High");
    expect(chip("Permission mode").getAttribute("aria-label")).toBe("Permission mode: Manual");
    expect(statusLine()).toBeNull();
    expect(words().disabled).toBe(false);
    expect(words().value).toBe("Fix the build");
  });
});

describe("a host where Ersa can't find a backend", () => {
  // verifies: FLOW-NEW-SESSION-R52
  it("names the backend when the host lists one alone, says Ersa looks again, and never says none is installed", async () => {
    await start(fakeCore([claudeCode("missing")]));
    expect(statusLine()?.textContent).toBe(
      "Ersa can't find Claude Code on this host. Install it, and Ersa looks again when you come back to this window.",
    );
    expect(document.body.textContent).not.toMatch(/No backend is available|not installed|Install a backend/i);
    fireEvent.click(chip("Backend"));
    expect(screen.getByRole("option", { name: /Claude Code/ }).textContent).toContain("Not found");
  });

  // verifies: FLOW-NEW-SESSION-R52
  it("says it can't find a backend when the host lists several, each Not found in the chip's list", async () => {
    await start(fakeCore([claudeCode("missing"), copilot("missing")]));
    expect(statusLine()?.textContent).toBe(
      "Ersa can't find a backend on this host. Install one, and Ersa looks again when you come back to this window.",
    );
    expect(document.body.textContent).not.toMatch(/No backend is available|not installed|Install a backend/i);
    fireEvent.click(chip("Backend"));
    for (const name of [/Claude Code/, /GitHub Copilot/]) expect(screen.getByRole("option", { name }).textContent).toContain("Not found");
  });
});
