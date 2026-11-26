// @vitest-environment jsdom
// Signing in to a backend from the new-session screen (FLOW-SIGN-IN), against a fake Core and a stand-in for opening
// a web address: the note, Sign in and Cancel, the page opened only over https, the backends read again on Core's
// answer and on a sign-in's end, each ending's words and how long they stay, and the note going once the backend can
// run. No real sign-in is started; the addresses are placeholders.

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, CoreEvent, Method } from "./core/client";
import type { ShellControl } from "./core/shell";
import type { BackendInfo, BackendSignInEndedNotification, CoreHelloResult } from "./protocol/core-api.gen";

afterEach(cleanup);

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const PAGE = "https://sign-in.example/start";

const claudeCode = (status: BackendInfo["status"], signingIn = false): BackendInfo => ({
  name: "claude-code",
  label: "Claude Code",
  version: "2.1.316",
  status,
  ...(signingIn ? { signing_in: { url: PAGE } } : {}),
  tested: { min: "2.1.0", max: "2.1.400" },
  models: [{ id: "default", label: "Default (recommended)", default: true, resolved: "claude-opus-5-5", efforts: ["high"] }],
  efforts: [{ id: "high", label: "High", default: true }],
  permission_modes: [{ id: "default", label: "Manual", default: true }],
});

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

type Answer = (params: never) => unknown;

/**
 * A fake Core: `backend.list` answers with `core.listing` (a listing, or a function called for each reading),
 * `backend.sign_in` and `backend.cancel_sign_in` as the test sets them, and `notify` sends a notification.
 */
function fakeCore(listing: BackendInfo[] | (() => Promise<BackendInfo[]>)) {
  const calls: { method: Method; params: unknown }[] = [];
  const core = {
    listing,
    signIn: (() => ({ url: PAGE })) as Answer,
    cancel: (() => ({})) as Answer,
  };
  let onEvent: (event: CoreEvent) => void = () => {};
  const answers: Partial<Record<Method, Answer>> = {
    "core.status": () => ({ sessions_running: 0, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => [{ id: "alpha", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "none", trusted: true }],
    "session.list": () => [],
    "workspace.list": () => [{ id: "w1", project_id: "alpha", kind: "root", path: "D:/code/alpha", state: "ready" }],
    "backend.list": () => (typeof core.listing === "function" ? core.listing() : core.listing),
    "backend.sign_in": (params) => core.signIn(params),
    "backend.cancel_sign_in": (params) => core.cancel(params),
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
  const ended = (ending: Omit<BackendSignInEndedNotification, "host_id" | "name">) =>
    act(async () => onEvent({ kind: "notification", method: "backend.sign_in_ended", params: { host_id: "local", name: "claude-code", ...ending } }));
  const called = (method: Method) => calls.filter((c) => c.method === method);
  return Object.assign(core, { client, ended, called, readings: () => called("backend.list").length });
}

const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));
const settled = async () => {
  for (let i = 0; i < 6; i += 1) await settle();
};

let back = new Set<() => void>();
const openLink = vi.fn<(url: string) => void>();
const shell = {
  onWindowBack: (handler: () => void) => {
    back.add(handler);
    return () => back.delete(handler);
  },
  onQuitRequested: () => () => {},
  onDragDrop: () => () => {},
  openLink,
} as unknown as ShellControl;
afterEach(() => {
  back = new Set();
  openLink.mockReset();
});

async function start(core: ReturnType<typeof fakeCore>) {
  render(createElement(App, { client: core.client, shell }));
  await settled();
}

const windowBack = async () => {
  await act(async () => {
    for (const handler of [...back]) handler();
  });
  await settled();
};

const note = () => document.querySelector<HTMLElement>(".sign-in-note");
const noteWords = () => note()?.querySelector(".sign-in-words")?.textContent;
const noteButton = () => note()?.querySelector("button") as HTMLButtonElement;
const shownLabel = () => noteButton().querySelector(".button-label[data-shown]")?.textContent;
const words = () => screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
const controls = () => screen.getByRole("group", { name: "Session controls" });
const chip = (name: string) => within(controls()).getByRole("button", { name: new RegExp(`^${name}:`) });
const signIn = async () => {
  fireEvent.click(noteButton());
  await settled();
};

const SIGNED_OUT = "Claude Code isn't signed in on this host. Sign in to start a session.";
const WAITING = "Waiting for you to sign in to Claude Code in your browser…";

describe("the sign-in note on the new-session screen", () => {
  // verifies: FLOW-SIGN-IN-R01
  it("stands above the composer for the first signed-out backend while none can run, the composer unavailable", async () => {
    const copilot = { ...claudeCode("signed_out"), name: "copilot", label: "GitHub Copilot" };
    const core = fakeCore([{ ...claudeCode("missing"), version: undefined }, copilot, claudeCode("signed_out")]);
    await start(core);
    expect(noteWords()).toBe("GitHub Copilot isn't signed in on this host. Sign in to start a session.");
    expect(shownLabel()).toBe("Sign in");
    expect(words().disabled).toBe(true);
    // Above the composer: in the room above the box, before it in the column.
    const box = screen.getByRole("form", { name: "Composer" });
    expect(note()!.compareDocumentPosition(box) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  // verifies: FLOW-SIGN-IN-R01
  it("shows no note while another backend can run", async () => {
    const core = fakeCore([{ ...claudeCode("available"), name: "copilot", label: "GitHub Copilot" }, claudeCode("signed_out")]);
    await start(core);
    expect(note()).toBeNull();
    expect(words().disabled).toBe(false);
  });

  it("keeps the status line beneath the note, saying what it says, as for a backend that isn't working", async () => {
    const broken = { ...claudeCode("error"), name: "copilot", label: "GitHub Copilot" };
    const core = fakeCore([claudeCode("signed_out"), broken]);
    await start(core);
    expect(noteWords()).toBe(SIGNED_OUT);
    const line = document.querySelector(".composer-status-line");
    expect(line?.textContent).toBe("GitHub Copilot isn't working on this host. Check that it's installed and signed in, then open New session again.");
    expect(note()!.compareDocumentPosition(line!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  // verifies: FLOW-SIGN-IN-R03
  it("asks Core to start the sign-in, reading Starting… until the backends read on Core's answer have arrived", async () => {
    const core = fakeCore([claudeCode("signed_out")]);
    await start(core);
    const answer = deferred<{ url?: string }>();
    core.signIn = () => answer.promise;
    await signIn();
    expect(core.called("backend.sign_in").map((c) => c.params)).toEqual([{ host_id: "local", name: "claude-code" }]);
    expect(shownLabel()).toBe("Starting…");
    expect(noteButton().getAttribute("aria-disabled")).toBe("true");
    // Chosen again while it starts: nothing more is asked.
    fireEvent.click(noteButton());
    await settled();
    expect(core.called("backend.sign_in")).toHaveLength(1);
    // Core answers; the reading on its answer is held, and Sign in stays unavailable meanwhile.
    const reading = deferred<BackendInfo[]>();
    core.listing = () => reading.promise;
    await act(async () => answer.resolve({ url: PAGE }));
    await settled();
    expect(shownLabel()).toBe("Starting…");
    expect(noteWords()).toBe(SIGNED_OUT);
    await act(async () => reading.resolve([claudeCode("signed_out", true)]));
    await settled();
    expect(shownLabel()).toBe("Cancel");
  });

  // verifies: FLOW-SIGN-IN-R03
  it("says the sign-in couldn't be started, with Core's reason, and offers Sign in when Core refuses", async () => {
    const core = fakeCore([claudeCode("signed_out")]);
    await start(core);
    core.signIn = () => Promise.reject({ kind: "rpc", error: { code: -32000, message: "Ersa can't start a sign-in to Claude Code.", data: { kind: "invalid_state" } } });
    await signIn();
    expect(noteWords()).toBe("Claude Code's sign-in couldn't be started. Ersa can't start a sign-in to Claude Code.");
    expect(shownLabel()).toBe("Sign in");
    expect(noteButton().getAttribute("aria-disabled")).toBeNull();
    expect(openLink).not.toHaveBeenCalled();
  });

  // verifies: FLOW-SIGN-IN-R04
  it("opens an https address Core answers with in the system's default browser", async () => {
    const core = fakeCore([claudeCode("signed_out")]);
    await start(core);
    await signIn();
    expect(openLink).toHaveBeenCalledWith(PAGE);
    expect(openLink).toHaveBeenCalledTimes(1);
    expect(core.called("backend.cancel_sign_in")).toHaveLength(0);
  });

  // verifies: FLOW-SIGN-IN-R05
  it.each(["http://sign-in.example/start", "javascript:alert(1)", "file:///C:/Windows/notepad.exe", "not an address"])(
    "opens nothing for %s, asks Core to stop the sign-in, and says it couldn't open the page",
    async (url) => {
      const core = fakeCore([claudeCode("signed_out")]);
      await start(core);
      core.signIn = () => ({ url });
      await signIn();
      expect(openLink).not.toHaveBeenCalled();
      expect(core.called("backend.cancel_sign_in").map((c) => c.params)).toEqual([{ host_id: "local", name: "claude-code" }]);
      expect(noteWords()).toBe("Ersa couldn't open Claude Code's sign-in page.");
      expect(shownLabel()).toBe("Sign in");
    },
  );

  // verifies: FLOW-SIGN-IN-R06
  it("says it waits for the person in their browser while the listing reports a sign-in under way, offering Cancel", async () => {
    // Under way from another window: this one shows it once it reads the host's backends.
    const core = fakeCore([claudeCode("signed_out", true)]);
    await start(core);
    expect(noteWords()).toBe(WAITING);
    expect(shownLabel()).toBe("Cancel");
  });

  // verifies: FLOW-SIGN-IN-R07
  it("asks Core to stop the sign-in on Cancel, and says again that the backend isn't signed in, offering Sign in", async () => {
    const core = fakeCore([claudeCode("signed_out", true)]);
    await start(core);
    const stopped = deferred<object>();
    core.cancel = () => stopped.promise;
    fireEvent.click(noteButton());
    await settled();
    expect(core.called("backend.cancel_sign_in").map((c) => c.params)).toEqual([{ host_id: "local", name: "claude-code" }]);
    expect(noteWords()).toBe(SIGNED_OUT);
    expect(shownLabel()).toBe("Sign in");
    // Held until Core has answered, so a Sign in never joins the sign-in on its way out.
    expect(noteButton().getAttribute("aria-disabled")).toBe("true");
    core.listing = [claudeCode("signed_out")];
    await act(async () => stopped.resolve({}));
    await settled();
    expect(noteButton().getAttribute("aria-disabled")).toBeNull();
    expect(core.called("backend.sign_in")).toHaveLength(0);
  });

  // verifies: FLOW-SIGN-IN-R09
  it("reads the host's backends again when Core answers that a sign-in has started, and when one ends", async () => {
    const core = fakeCore([claudeCode("signed_out")]);
    await start(core);
    const before = core.readings();
    core.listing = [claudeCode("signed_out", true)];
    await signIn();
    expect(core.readings()).toBe(before + 1);
    expect(shownLabel()).toBe("Cancel");
    core.listing = [claudeCode("signed_out")];
    await core.ended({ outcome: "canceled" });
    await settled();
    expect(core.readings()).toBe(before + 2);
    expect(shownLabel()).toBe("Sign in");
  });

  // verifies: FLOW-SIGN-IN-R09
  it("reads again on Core's answer even while a reading is already out, which may not see the sign-in", async () => {
    const core = fakeCore([claudeCode("signed_out")]);
    await start(core);
    const early = deferred<BackendInfo[]>();
    core.listing = () => early.promise;
    // The window comes back, and its reading is still out when Core answers the Sign in.
    await act(async () => {
      for (const handler of [...back]) handler();
    });
    const before = core.readings();
    await signIn();
    core.listing = [claudeCode("signed_out", true)];
    await act(async () => early.resolve([claudeCode("signed_out")]));
    await settled();
    expect(core.readings()).toBe(before + 1);
    expect(shownLabel()).toBe("Cancel");
  });

  // verifies: FLOW-SIGN-IN-R10
  it("goes when the listing reports the backend can run, and stops a sign-in the listing still reports under way", async () => {
    const core = fakeCore([claudeCode("signed_out", true)]);
    await start(core);
    expect(noteWords()).toBe(WAITING);
    // Signed in in a terminal meanwhile: the window comes back and reads the backends.
    core.listing = [claudeCode("available", true)];
    await windowBack();
    expect(note()).toBeNull();
    expect(core.called("backend.cancel_sign_in").map((c) => c.params)).toEqual([{ host_id: "local", name: "claude-code" }]);
  });

  // verifies: FLOW-SIGN-IN-R10
  it("asks nothing more of Core when the backend can run with no sign-in under way", async () => {
    const core = fakeCore([claudeCode("signed_out")]);
    await start(core);
    core.listing = [claudeCode("available")];
    await windowBack();
    expect(note()).toBeNull();
    expect(core.called("backend.cancel_sign_in")).toHaveLength(0);
  });

  // verifies: FLOW-SIGN-IN-R11
  it("chooses the backend once it can run, makes the composer available with focus in its words, and sends nothing", async () => {
    const core = fakeCore([claudeCode("signed_out")]);
    await start(core);
    core.listing = [claudeCode("signed_out", true)];
    await signIn();
    noteButton().focus();
    core.listing = [claudeCode("available")];
    await core.ended({ outcome: "signed_in" });
    await settled();
    expect(note()).toBeNull();
    expect(words().disabled).toBe(false);
    await waitFor(() => expect(document.activeElement).toBe(words()));
    expect(chip("Backend").textContent).toContain("Claude Code");
    expect(chip("Model").textContent).toContain("Default");
    expect(core.called("session.create")).toHaveLength(0);
    expect(core.called("session.send")).toHaveLength(0);
  });

  // verifies: FLOW-SIGN-IN-R12
  it("says the sign-in finished but the backend still reports it isn't signed in, offering Sign in", async () => {
    const core = fakeCore([claudeCode("signed_out", true)]);
    await start(core);
    core.listing = [claudeCode("signed_out")];
    await core.ended({ outcome: "signed_in" });
    await settled();
    expect(noteWords()).toBe("The sign-in finished, but Claude Code still reports that it isn't signed in.");
    expect(shownLabel()).toBe("Sign in");
  });

  // verifies: FLOW-SIGN-IN-R13
  it("says the sign-in didn't finish, then the backend's reason, offering Sign in", async () => {
    const core = fakeCore([claudeCode("signed_out", true)]);
    await start(core);
    core.listing = [claudeCode("signed_out")];
    await core.ended({ outcome: "failed", message: "The account has no plan that includes Claude Code." });
    await settled();
    expect(noteWords()).toBe("Claude Code's sign-in didn't finish. The account has no plan that includes Claude Code.");
    expect(shownLabel()).toBe("Sign in");
  });

  // verifies: FLOW-SIGN-IN-R13
  it("says the sign-in didn't finish alone when Core gives no reason", async () => {
    const core = fakeCore([claudeCode("signed_out", true)]);
    await start(core);
    core.listing = [claudeCode("signed_out")];
    await core.ended({ outcome: "failed" });
    await settled();
    expect(noteWords()).toBe("Claude Code's sign-in didn't finish.");
  });

  // verifies: FLOW-SIGN-IN-R14
  it("says a sign-in that timed out took too long and was stopped, and adds nothing for one canceled", async () => {
    const core = fakeCore([claudeCode("signed_out", true)]);
    await start(core);
    core.listing = [claudeCode("signed_out")];
    await core.ended({ outcome: "timed_out" });
    await settled();
    expect(noteWords()).toBe("Claude Code's sign-in took too long and was stopped.");
    expect(shownLabel()).toBe("Sign in");

    cleanup();
    const other = fakeCore([claudeCode("signed_out", true)]);
    await start(other);
    other.listing = [claudeCode("signed_out")];
    await other.ended({ outcome: "canceled" });
    await settled();
    expect(noteWords()).toBe(SIGNED_OUT);
  });

  // verifies: FLOW-SIGN-IN-R15
  it("keeps a failure's words through later readings until Sign in is chosen", async () => {
    const core = fakeCore([claudeCode("signed_out", true)]);
    await start(core);
    core.listing = [claudeCode("signed_out")];
    await core.ended({ outcome: "timed_out" });
    await settled();
    await windowBack();
    await windowBack();
    expect(noteWords()).toBe("Claude Code's sign-in took too long and was stopped.");
    // Chosen again: gone, and not back once the start has settled with the backend still signed out.
    core.signIn = () => ({});
    await signIn();
    expect(shownLabel()).toBe("Sign in");
    expect(noteWords()).toBe(SIGNED_OUT);
  });

  // verifies: FLOW-SIGN-IN-R15
  it("drops a failure's words once the note goes, so they don't come back with it", async () => {
    const core = fakeCore([claudeCode("signed_out")]);
    await start(core);
    core.signIn = () => Promise.reject({ kind: "rpc", error: { code: -32000, message: "No.", data: { kind: "invalid_state" } } });
    await signIn();
    expect(noteWords()).toBe("Claude Code's sign-in couldn't be started. No.");
    core.listing = [claudeCode("available")];
    await windowBack();
    expect(note()).toBeNull();
    core.listing = [claudeCode("signed_out")];
    await windowBack();
    expect(noteWords()).toBe(SIGNED_OUT);
  });
});
