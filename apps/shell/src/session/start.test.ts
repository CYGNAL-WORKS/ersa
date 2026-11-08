import { describe, expect, it } from "vitest";
import type { BridgeError, CoreClient, Method, Params } from "../core/client";
import type { SessionValues } from "./controls";
import { StartStop, lastUsedFrom, readChoices, rememberPatch, remembered, startSession } from "./start";

/** A client that records each call and answers from `answers`; a method without an answer fails the test. */
function fakeClient(answers: Partial<Record<Method, (params: never) => unknown>> = {}) {
  const calls: { method: Method; params: unknown }[] = [];
  const client: Pick<CoreClient, "call"> = {
    call: <M extends Method>(method: M, params?: Params<M>) => {
      calls.push({ method, params });
      const answer = answers[method];
      if (!answer) return Promise.reject(new Error(`unexpected ${method}`)) as never;
      try {
        return Promise.resolve(answer(params as never)) as never;
      } catch (error) {
        return Promise.reject(error) as never;
      }
    },
  };
  return { client, calls, methods: () => calls.map((c) => c.method) };
}

const refusal = (message: string, kind = "invalid_state"): BridgeError => ({ kind: "rpc", error: { code: -32000, message, data: { kind } } as never });

const values = (workspace: SessionValues["workspace"]): SessionValues => ({
  host_id: "local",
  project_id: "p1",
  workspace,
  backend: "claude-code",
  model: "opus",
  effort: "high",
  permission_mode: "default",
});

const root = { id: "w-root", project_id: "p1", kind: "root", path: "D:\\p", vcs_ref: "", state: "ready" };
const isolated = { id: "w-new", project_id: "p1", kind: "isolated", path: "D:\\ws\\p-1", vcs_ref: "ersa/p-1", state: "ready" };
const session = { id: "s1", workspace_id: "w-new" };

const working = {
  "workspace.create": () => isolated,
  "workspace.list": () => [root, isolated],
  "session.create": (p: { workspace_id: string }) => ({ ...session, workspace_id: p.workspace_id }),
  "session.send": () => ({ turn: 1 }),
};

describe("starting a session from the new-session screen", () => {
  // verifies: FLOW-NEW-SESSION-R04
  it("creates a workspace, starts the session in it with the chosen values, and sends the first message", async () => {
    const { client, calls, methods } = fakeClient(working);
    const outcome = await startSession(client, values("isolated"), "Fix the build");
    expect(methods()).toEqual(["workspace.create", "session.create", "session.send"]);
    expect(calls[0]?.params).toEqual({ project_id: "p1" });
    expect(calls[1]?.params).toEqual({
      workspace_id: "w-new",
      backend: "claude-code",
      model: "opus",
      effort: "high",
      permission_mode: "default",
    });
    expect(calls[2]?.params).toEqual({ id: "s1", text: "Fix the build" });
    expect(outcome).toMatchObject({ ok: true, session: { id: "s1" } });
  });

  // verifies: FLOW-NEW-SESSION-R21
  it("creates the workspace from the chosen base branch", async () => {
    const { client, calls } = fakeClient(working);
    await startSession(client, { ...values("isolated"), base: "release/1.0" }, "Fix the build");
    expect(calls[0]).toEqual({ method: "workspace.create", params: { project_id: "p1", base: "release/1.0" } });
  });

  // verifies: FLOW-NEW-SESSION-R05
  it("starts the session in the project's root workspace and creates no workspace", async () => {
    const { client, calls, methods } = fakeClient(working);
    const outcome = await startSession(client, values("root"), "What does this do?");
    expect(methods()).toEqual(["workspace.list", "session.create", "session.send"]);
    expect(calls[1]?.params).toMatchObject({ workspace_id: "w-root" });
    expect(outcome).toMatchObject({ ok: true, session: { workspace_id: "w-root" } });
  });

  it("leaves out a value the backend offers no choice of", async () => {
    const { client, calls } = fakeClient(working);
    await startSession(client, { ...values("isolated"), effort: undefined }, "hello");
    expect(calls[1]?.params).not.toHaveProperty("effort");
  });

  // verifies: FLOW-NEW-SESSION-R08
  it("on a backend that fails to start, reports its own message and removes what it made", async () => {
    const { client, calls, methods } = fakeClient({
      ...working,
      "session.send": () => {
        throw refusal("session.send: the backend could not be started: claude: not logged in");
      },
      "session.list": () => [{ ...session, state: "error" }],
      "session.stop": () => session,
      "session.remove": () => session,
      "workspace.remove": () => ({ ...isolated, state: "removing" }),
    });
    const outcome = await startSession(client, values("isolated"), "Fix the build");
    expect(outcome).toEqual({ ok: false, message: "the backend could not be started: claude: not logged in" });
    // The session goes before its workspace: Core refuses to remove a workspace that has one.
    expect(methods()).toEqual(["workspace.create", "session.create", "session.send", "session.list", "session.stop", "session.remove", "workspace.remove"]);
    expect(calls.at(-1)?.params).toEqual({ id: "w-new" });
  });

  // verifies: FLOW-NEW-SESSION-R08
  it("never removes the root workspace", async () => {
    const { client, methods } = fakeClient({
      ...working,
      "session.send": () => {
        throw refusal("session.send: the backend could not be started: exited");
      },
      "session.list": () => [{ ...session, state: "error" }],
      "session.stop": () => session,
      "session.remove": () => session,
    });
    const outcome = await startSession(client, values("root"), "hello");
    expect(outcome.ok).toBe(false);
    expect(methods()).not.toContain("workspace.remove");
  });

  // verifies: FLOW-NEW-SESSION-R08
  it("keeps a session still starting when the first message timed out, and gives the text back", async () => {
    const { client, methods } = fakeClient({
      ...working,
      "session.send": () => {
        throw refusal("session.send: the session is still starting; try again shortly");
      },
      "session.list": () => [{ ...session, state: "starting" }],
    });
    const outcome = await startSession(client, values("isolated"), "Fix the build");
    expect(outcome).toMatchObject({
      ok: true,
      session: { id: "s1", state: "starting" },
      workspace: { id: "w-new" },
      firstSend: { sending: { state: "failed", message: "the session is still starting; try again shortly" }, giveBack: true },
    });
    expect(methods()).not.toContain("session.stop");
    expect(methods()).not.toContain("workspace.remove");
  });

  // verifies: FLOW-NEW-SESSION-R08
  it("keeps a running session whose first message Core already passed on, and keeps the text there", async () => {
    const { client, methods } = fakeClient({
      ...working,
      "session.send": () => {
        throw refusal("session.send: the message was sent, but Core could not project it in time", "internal");
      },
      "session.list": () => [{ ...session, state: "running" }],
    });
    const outcome = await startSession(client, values("isolated"), "Fix the build");
    expect(outcome).toMatchObject({ ok: true, firstSend: { giveBack: false } });
    expect(methods()).not.toContain("session.remove");
  });

  // verifies: FLOW-NEW-SESSION-R08
  it("keeps a session whose backend has the first message even when it has exited since", async () => {
    const { client, methods } = fakeClient({
      ...working,
      "session.send": () => {
        throw refusal("session.send: the message was sent, but Core could not project it in time", "internal");
      },
      "session.list": () => [{ ...session, state: "error" }],
    });
    const outcome = await startSession(client, values("isolated"), "Fix the build");
    expect(outcome).toMatchObject({ ok: true, firstSend: { giveBack: false } });
    expect(methods()).not.toContain("session.remove");
    expect(methods()).not.toContain("workspace.remove");
  });

  it("keeps the session when Core cannot say whether it ran", async () => {
    const { client, methods } = fakeClient({
      ...working,
      "session.send": () => {
        throw { kind: "unavailable", message: "Core is not connected." };
      },
    });
    const outcome = await startSession(client, values("isolated"), "Fix the build");
    expect(outcome).toMatchObject({ ok: true, firstSend: { sending: { state: "failed", message: "Core is not connected." }, giveBack: true } });
    expect(methods()).not.toContain("session.remove");
  });

  // verifies: FLOW-NEW-SESSION-R08
  it("removes the workspace when the session could not be created, and keeps the first failure's message", async () => {
    const { client, methods } = fakeClient({
      ...working,
      "session.create": () => {
        throw refusal("session.create: model \"x\" is not one this backend offers");
      },
      "workspace.remove": () => {
        throw refusal("workspace.remove: busy");
      },
    });
    const outcome = await startSession(client, values("isolated"), "hello");
    expect(outcome).toEqual({ ok: false, message: 'model "x" is not one this backend offers' });
    expect(methods()).toEqual(["workspace.create", "session.create", "workspace.remove"]);
  });

  it("reports a project with no root workspace listed rather than starting elsewhere", async () => {
    const { client, methods } = fakeClient({ ...working, "workspace.list": () => [isolated] });
    const outcome = await startSession(client, values("root"), "hello");
    expect(outcome.ok).toBe(false);
    expect(methods()).toEqual(["workspace.list"]);
  });

  it("starts nothing without a backend to run", async () => {
    const { client, methods } = fakeClient(working);
    const outcome = await startSession(client, { ...values("isolated"), backend: undefined }, "hello");
    expect(outcome.ok).toBe(false);
    expect(methods()).toEqual([]);
  });
});

describe("stop while a new session starts", () => {
  // verifies: FLOW-SEND-MESSAGE-R03
  it("cancels the first message while the backend starts, and takes back the session and workspace it made", async () => {
    let refuse: (error: BridgeError) => void = () => {};
    const { client, methods } = fakeClient({
      ...working,
      "session.send": () => new Promise((_, reject) => (refuse = reject)),
      "session.interrupt": () => ({}),
      "session.stop": () => ({}),
      "session.remove": () => ({}),
      "workspace.remove": () => ({}),
    });
    const stop = new StartStop(client);
    const started = startSession(client, values("isolated"), "Fix the build", stop);
    await new Promise((r) => setTimeout(r, 0));
    stop.stop();
    refuse(refusal("the message was stopped before it was sent"));
    expect(await started).toEqual({ ok: false, stopped: true });
    expect(methods()).toEqual([
      "workspace.create",
      "session.create",
      "session.send",
      "session.interrupt",
      "session.stop",
      "session.remove",
      "workspace.remove",
    ]);
  });

  // verifies: FLOW-SEND-MESSAGE-R03
  it("keeps a session whose backend already has the message, even after a stop", async () => {
    let refuse: (error: BridgeError) => void = () => {};
    const { client, methods } = fakeClient({
      ...working,
      "session.send": () => new Promise((_, reject) => (refuse = reject)),
      "session.interrupt": () => ({}),
      "session.list": () => [{ ...session, state: "running" }],
    });
    const stop = new StartStop(client);
    const started = startSession(client, values("isolated"), "Fix the build", stop);
    await new Promise((r) => setTimeout(r, 0));
    stop.stop();
    refuse(refusal("the backend took the message and stopped", "internal"));
    expect(await started).toMatchObject({ ok: true, session: { id: "s1" } });
    expect(methods()).not.toContain("session.remove");
    expect(methods()).not.toContain("workspace.remove");
  });

  // verifies: FLOW-SEND-MESSAGE-R03
  it("sends nothing when stopped before the session exists", async () => {
    const { client, methods } = fakeClient({ ...working, "session.stop": () => ({}), "session.remove": () => ({}), "workspace.remove": () => ({}) });
    const stop = new StartStop(client);
    stop.stop();
    expect(await startSession(client, values("isolated"), "Fix the build", stop)).toEqual({ ok: false, stopped: true });
    expect(methods()).not.toContain("session.send");
    expect(methods()).toContain("workspace.remove");
  });
});

describe("opening the new-session screen", () => {
  // verifies: FLOW-NEW-SESSION-R03
  it("only reads: the host's backends and the remembered values", async () => {
    const { client, methods } = fakeClient({ "backend.list": () => [], "settings.get": () => rememberPatch(values("root")) });
    // The window reads the settings once and shares them with the rail.
    const choices = await readChoices(client, "local", client.call("settings.get"));
    expect(methods().sort()).toEqual(["backend.list", "settings.get"]);
    expect(choices).toEqual({ backends: [], lastUsed: { p1: values("root") } });
  });

  it("offers no backend when they could not be read, and starts at the defaults without remembered values", async () => {
    const { client } = fakeClient({});
    expect(await readChoices(client, "local", client.call("settings.get"))).toEqual({ backends: [], lastUsed: {} });
  });
});

describe("the values remembered for a project", () => {
  it("are stored under the project in Core's settings and read back from them", () => {
    const patch = rememberPatch(values("root"));
    expect(patch).toEqual({ new_session: { last_used: { p1: values("root") } } });
    expect(lastUsedFrom(patch)).toEqual({ p1: values("root") });
  });

  it("clear a value the new start left out, since Core merges the patch key by key", () => {
    const patch = rememberPatch({ ...values("root"), model: undefined, effort: undefined });
    expect(patch.new_session.last_used.p1).toMatchObject({ model: null, effort: null, permission_mode: "default" });
    expect(lastUsedFrom(patch).p1).toEqual({ ...values("root"), model: undefined, effort: undefined });
  });

  // verifies: FLOW-NEW-SESSION-R21
  it("never hold the base branch: each new session starts at the default branch", () => {
    const patch = rememberPatch({ ...values("isolated"), base: "release/1.0" });
    expect(patch.new_session.last_used.p1).not.toHaveProperty("base");
    expect(remembered({ ...values("isolated"), base: "release/1.0" })).toEqual(values("isolated"));
    expect(lastUsedFrom({ new_session: { last_used: { p1: { ...values("isolated"), base: "release/1.0" } } } }).p1).toEqual(values("isolated"));
  });

  it("read as none from settings that hold none, or hold something unreadable", () => {
    expect(lastUsedFrom({})).toEqual({});
    expect(lastUsedFrom({ new_session: "x" })).toEqual({});
    expect(lastUsedFrom({ new_session: { last_used: { p1: { workspace: "elsewhere" }, p2: values("root") } } })).toEqual({
      p2: { ...values("root"), project_id: "p2" },
    });
  });
});

describe("a first message with attachments", () => {
  // verifies: FLOW-SEND-MESSAGE-R13
  it("sends them with the first message", async () => {
    const { client, calls } = fakeClient(working);
    await startSession(client, values("isolated"), "Look", undefined, [{ kind: "folder", path: "D:\p\src" }]);
    expect(calls.find((c) => c.method === "session.send")?.params).toEqual({ id: "s1", text: "Look", attachments: [{ kind: "folder", path: "D:\p\src" }] });
  });

  // verifies: FLOW-SEND-MESSAGE-R10
  // verifies: FLOW-SEND-MESSAGE-R15
  it("keeps the session and says which attachment Core refused, or that Ersa can't tell where the message went", async () => {
    const refused: BridgeError = { kind: "rpc", error: { code: -32000, message: "session.send: refused", data: { kind: "attachment_refused", detail: "Too large.", index: 0 } } };
    const alive = { "session.list": () => [{ id: "s1", state: "idle" }] };
    const a = await startSession(fakeClient({ ...working, ...alive, "session.send": () => Promise.reject(refused) }).client, values("isolated"), "Look", undefined, [{ kind: "image", path: "D:\big.png" }]);
    expect(a).toMatchObject({ ok: true, firstSend: { sending: { state: "failed", message: "Too large.", index: 0 }, giveBack: true } });
    const lost: BridgeError = { kind: "unavailable", message: "Ersa got no answer in time.", sent: true };
    const b = await startSession(fakeClient({ ...working, ...alive, "session.send": () => Promise.reject(lost) }).client, values("isolated"), "Look");
    expect(b).toMatchObject({ ok: true, firstSend: { sending: { state: "unknown" }, giveBack: true } });
    // Handed over just before the backend stopped: the session is kept, though it reads as an error.
    const handed: BridgeError = { kind: "rpc", error: { code: -32000, message: "session.send: handed over", data: { kind: "delivery_unknown" } } };
    const stopped = fakeClient({ ...working, "session.list": () => [{ id: "s1", state: "error" }], "session.send": () => Promise.reject(handed) });
    const c = await startSession(stopped.client, values("isolated"), "Look");
    expect(c).toMatchObject({ ok: true, firstSend: { sending: { state: "unknown" } } });
    expect(stopped.methods()).not.toContain("session.remove");
  });
});
