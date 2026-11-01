import { describe, expect, it } from "vitest";
import type { BridgeError, CoreClient, Method, Params } from "../core/client";
import { type SessionView, follow } from "./follow";
import { events } from "./replays";

const settle = () => new Promise((r) => setTimeout(r, 0));

/** A client that records each call and answers from `answers`; unanswered calls never settle. */
function fakeClient(answers: Partial<Record<Method, (params: unknown) => Promise<unknown>>> = {}) {
  const calls: { method: Method; params: unknown }[] = [];
  const client: CoreClient = {
    attach: () => new Promise(() => {}),
    call: <M extends Method>(method: M, params?: Params<M>) => {
      calls.push({ method, params });
      return (answers[method]?.(params) ?? new Promise(() => {})) as never;
    },
  };
  return { client, calls };
}

/** What the replay server answers every request that would change its session. */
const replayRefusal = (method: string): BridgeError => ({
  kind: "rpc",
  error: {
    code: -32000,
    message: `${method}: the session is a replay of a recording and cannot change`,
    data: { kind: "invalid_state" },
  },
});

const replay = events("11-stream-permission-prompt-tool-stdio");

function following(answers: Parameters<typeof fakeClient>[0] = {}) {
  const { client, calls } = fakeClient({ "session.subscribe": () => Promise.resolve({}), ...answers });
  const seen: SessionView[] = [];
  const follower = follow(client, "s-replay", (v) => seen.push(v));
  return { follower, calls, seen, last: () => seen.at(-1) as SessionView };
}

describe("follow", () => {
  it("subscribes from the start, then from after the last event on each new connection", async () => {
    const { follower, calls, last } = following();
    follower.attached();
    await settle();
    expect(last().stream).toEqual({ state: "live" });
    for (const event of replay.slice(0, 8)) follower.receive(event);
    follower.attached();
    expect(calls.filter((c) => c.method === "session.subscribe").map((c) => c.params)).toEqual([
      { id: "s-replay", from_seq: 0 },
      { id: "s-replay", from_seq: 8 },
    ]);
  });

  it("subscribes again from a gap on a live subscription, once per gap", async () => {
    const { follower, calls } = following();
    follower.attached();
    await settle();
    for (const event of replay.slice(0, 5)) follower.receive(event);
    // Event 5 was lost on the way; 6 and 7 arrive.
    follower.receive(replay[6] as (typeof replay)[number]);
    follower.receive(replay[7] as (typeof replay)[number]);
    await settle();
    expect(calls.filter((c) => c.method === "session.subscribe").map((c) => c.params)).toEqual([
      { id: "s-replay", from_seq: 0 },
      { id: "s-replay", from_seq: 5 },
    ]);
  });

  it("applies only this session's events", () => {
    const { follower, last } = following();
    const other = { ...(replay[5] as (typeof replay)[number]), session_id: "s-other" };
    follower.receive(other);
    expect(last().transcript.items).toEqual([]);
  });

  it("shows nothing for a sent message until Core records it", async () => {
    const { follower, calls, last } = following({ "session.send": () => Promise.resolve({ turn: 1 }) });
    expect(await follower.send("hello")).toBe(true);
    expect(calls.at(-1)).toEqual({ method: "session.send", params: { id: "s-replay", text: "hello" } });
    expect(last().transcript.items).toEqual([]);
    expect(last().message).toBeUndefined();
  });

  it("keeps a refused message's reason", async () => {
    const { follower, last } = following({ "session.send": () => Promise.reject(replayRefusal("session.send")) });
    expect(await follower.send("hello")).toBe(false);
    expect(last().message).toEqual({ state: "failed", message: "session.send: the session is a replay of a recording and cannot change" });
  });

  it("answers a permission request, and the decision event settles the answer", async () => {
    const { follower, calls, last } = following({ "session.decide_permission": () => Promise.resolve({}) });
    const requested = replay.findIndex((e) => e.event.type === "permission.requested");
    for (const event of replay.slice(0, requested + 1)) follower.receive(event);
    const request = replay[requested]?.event;
    const id = request?.type === "permission.requested" ? request.request_id : "";

    follower.decide(id, "allow");
    expect(calls.at(-1)).toEqual({
      method: "session.decide_permission",
      params: { id: "s-replay", request_id: id, behavior: "allow" },
    });
    expect(last().answers[id]).toEqual({ state: "sending" });
    await settle();
    // Answered, but nothing is shown as decided until Core says so.
    expect(last().answers[id]).toEqual({ state: "sending" });

    for (const event of replay.slice(requested + 1)) follower.receive(event);
    expect(last().answers).toEqual({});
    const permission = last().transcript.items.find((i) => i.kind === "permission");
    expect(permission?.kind === "permission" && permission.decision?.behavior).toBe("allow");
  });

  it("shows why an answer was refused, and the request stays open", async () => {
    const { follower, last } = following({ "session.decide_permission": () => Promise.reject(replayRefusal("session.decide_permission")) });
    follower.decide("r1", "deny");
    await settle();
    expect(last().answers.r1).toEqual({
      state: "failed",
      message: "session.decide_permission: the session is a replay of a recording and cannot change",
    });
  });

  it("unsubscribes when stopped, and reports nothing after", () => {
    const { follower, calls, seen } = following({ "session.unsubscribe": () => Promise.resolve({}) });
    const before = seen.length;
    follower.stop();
    follower.receive(replay[0] as (typeof replay)[number]);
    expect(calls.at(-1)).toEqual({ method: "session.unsubscribe", params: { id: "s-replay" } });
    expect(seen.length).toBe(before);
  });
});
