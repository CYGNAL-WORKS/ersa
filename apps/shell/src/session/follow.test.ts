import { describe, expect, it } from "vitest";
import type { BridgeError, CoreClient, Method, Params } from "../core/client";
import { type SessionView, follow } from "./follow";
import { events } from "./replays";
import { EVENT_VERSION } from "./transcript";

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
  // Core's history holds the whole replay: subscribing never finds it shorter than what is shown.
  const lastSeq = replay.at(-1)?.seq;
  const { client, calls } = fakeClient({ "session.subscribe": () => Promise.resolve({ last_seq: lastSeq }), ...answers });
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
    const { follower, calls, last } = following();
    follower.attached();
    await settle();
    for (const event of replay.slice(0, 5)) follower.receive(event);
    // Event 5 was lost on the way; 6 arrives, and the follower subscribes again.
    follower.receive(replay[6] as (typeof replay)[number]);
    await settle();
    expect(last().stream).toEqual({ state: "live" });
    // Another event beyond the same gap, while live again: no second resubscribe; the stream says it failed.
    follower.receive(replay[7] as (typeof replay)[number]);
    await settle();
    expect(last().stream).toMatchObject({ state: "failed", gap: true });
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

  // verifies: FLOW-SEND-MESSAGE-R01
  it("shows nothing for a sent message until Core records it", async () => {
    const { follower, calls, last } = following({ "session.send": () => Promise.resolve({ turn: 1 }) });
    expect(await follower.send("hello")).toBe(true);
    expect(calls.at(-1)).toEqual({ method: "session.send", params: { id: "s-replay", text: "hello" } });
    expect(last().transcript.items).toEqual([]);
    expect(last().message).toBeUndefined();
  });

  // verifies: FLOW-SEND-MESSAGE-R05
  it("keeps a refused message's reason", async () => {
    const { follower, last } = following({ "session.send": () => Promise.reject(replayRefusal("session.send")) });
    expect(await follower.send("hello")).toBe(false);
    // The method name Core puts first means nothing to the user and is dropped.
    expect(last().message).toEqual({ state: "failed", message: "the session is a replay of a recording and cannot change" });
  });

  it("starts again from the beginning when Core's history is shorter than what is shown", async () => {
    // A replay restarted on the same endpoint: the second subscribe finds no events past 3.
    const answers = [{ last_seq: 21 }, { last_seq: 3 }, { last_seq: 3 }];
    const { follower, calls, last } = following({ "session.subscribe": () => Promise.resolve(answers.shift()) });
    follower.attached();
    await settle();
    for (const event of replay.slice(0, 10)) follower.receive(event);
    follower.attached();
    await settle();
    expect(calls.filter((c) => c.method === "session.subscribe").map((c) => c.params)).toEqual([
      { id: "s-replay", from_seq: 0 },
      { id: "s-replay", from_seq: 10 },
      { id: "s-replay", from_seq: 0 },
    ]);
    expect(last().transcript.lastSeq).toBe(-1);
    expect(last().stream).toEqual({ state: "live" });
  });

  // Two ways a client meets another view: Core restarted after turns were taken
  // in a terminal (its history is longer, but numbered afresh), or another
  // window opened the session afresh while this one was away. Either way the
  // numbers no longer line up with what is shown.
  for (const [why, answers] of [
    ["Core restarted with a longer history", [{ last_seq: 21, view: "v-1" }, { last_seq: 40, view: "v-2" }, { last_seq: 40, view: "v-2" }]],
    ["another window reopened the session", [{ last_seq: 21, view: "v-1" }, { last_seq: 21, view: "v-3" }, { last_seq: 21, view: "v-3" }]],
  ] as const) {
    it(`starts over when Core answers with another view: ${why}`, async () => {
      const queue: unknown[] = [...answers];
      const { follower, calls, last } = following({ "session.subscribe": () => Promise.resolve(queue.shift()) });
      follower.attached();
      await settle();
      for (const event of replay.slice(0, 10)) follower.receive(event);
      follower.attached();
      await settle();
      expect(calls.filter((c) => c.method === "session.subscribe").map((c) => c.params)).toEqual([
        { id: "s-replay", from_seq: 0 },
        { id: "s-replay", from_seq: 10 },
        { id: "s-replay", from_seq: 0 },
      ]);
      expect(last().transcript.lastSeq).toBe(-1);
      expect(last().transcript.items).toEqual([]);
      expect(last().stream).toEqual({ state: "live" });
      // The new view's events apply from the start.
      follower.receive(replay[0] as (typeof replay)[number]);
      expect(last().transcript.lastSeq).toBe(0);
    });
  }

  it("keeps what it shows when Core answers with the same view", async () => {
    const queue: unknown[] = [{ last_seq: 21, view: "v-1" }, { last_seq: 21, view: "v-1" }];
    const { follower, calls, last } = following({ "session.subscribe": () => Promise.resolve(queue.shift()) });
    follower.attached();
    await settle();
    for (const event of replay.slice(0, 10)) follower.receive(event);
    follower.attached();
    await settle();
    expect(calls.filter((c) => c.method === "session.subscribe")).toHaveLength(2);
    expect(last().transcript.lastSeq).toBe(9);
  });

  it("starts where the view does when it no longer holds what came between", async () => {
    const queue: unknown[] = [{ last_seq: 21, view: "v-1" }, { last_seq: 21, view: "v-1", first_seq: 15 }];
    const { follower, calls, last } = following({ "session.subscribe": () => Promise.resolve(queue.shift()) });
    follower.attached();
    await settle();
    for (const event of replay.slice(0, 10)) follower.receive(event);
    follower.attached();
    await settle();
    expect(calls.filter((c) => c.method === "session.subscribe")).toHaveLength(2);
    expect(last().transcript.lastSeq).toBe(14);
    expect(last().transcript.items).toEqual([
      { kind: "notice", key: "earlier:15", message: "Earlier events of this session are no longer held." },
    ]);
    expect(last().stream).toEqual({ state: "live" });
  });

  it("goes offline when the connection is lost, and live again once subscribed", async () => {
    const { follower, last } = following();
    follower.attached();
    await settle();
    follower.detached();
    expect(last().stream).toEqual({ state: "offline" });
    follower.attached();
    await settle();
    expect(last().stream).toEqual({ state: "live" });
  });

  it("survives an envelope without an event: a notice in its place, and the next event applies", () => {
    const { follower, last } = following();
    const broken = { v: EVENT_VERSION, session_id: "s-replay", seq: 0 } as unknown as (typeof replay)[number];
    follower.receive(broken);
    follower.receive(null as unknown as (typeof replay)[number]);
    follower.receive({ ...(replay[1] as (typeof replay)[number]) });
    expect(last().transcript.lastSeq).toBe(1);
    expect(last().transcript.items[0]).toMatchObject({ kind: "notice" });
  });

  it("stops subscribing once stopped, even when Core's history is shorter", async () => {
    const { follower, calls } = following({ "session.subscribe": () => Promise.resolve({ last_seq: 3 }), "session.unsubscribe": () => Promise.resolve({}) });
    for (const event of replay.slice(0, 10)) follower.receive(event);
    follower.attached();
    follower.stop();
    await settle();
    await settle();
    expect(calls.filter((c) => c.method === "session.subscribe")).toHaveLength(1);
  });

  it("starts again when Core lists no events for a session that shows some", async () => {
    const answers: unknown[] = [{ last_seq: 21 }, {}, {}];
    const { follower, calls, last } = following({ "session.subscribe": () => Promise.resolve(answers.shift()) });
    follower.attached();
    await settle();
    for (const event of replay.slice(0, 10)) follower.receive(event);
    follower.attached();
    await settle();
    expect(calls.filter((c) => c.method === "session.subscribe").map((c) => c.params)).toEqual([
      { id: "s-replay", from_seq: 0 },
      { id: "s-replay", from_seq: 10 },
      { id: "s-replay", from_seq: 0 },
    ]);
    expect(last().transcript.lastSeq).toBe(-1);
  });

  it("says the stream failed when subscribing again does not fill a gap", async () => {
    const { follower, calls, last } = following();
    follower.attached();
    await settle();
    for (const event of replay.slice(0, 5)) follower.receive(event);
    follower.receive(replay[6] as (typeof replay)[number]);
    await settle();
    // The backlog came without event 5; a later event arrives beyond the same gap.
    follower.receive(replay[8] as (typeof replay)[number]);
    expect(last().stream).toEqual({ state: "failed", message: "event 5 never arrived", gap: true });
    expect(calls.filter((c) => c.method === "session.subscribe")).toHaveLength(2);
    // The backlog was late, not lost: the missing event arrives, and the stream is live again.
    follower.receive(replay[5] as (typeof replay)[number]);
    expect(last().stream).toEqual({ state: "live" });
    expect(last().transcript.lastSeq).toBe(5);
  });

  it("keeps what it shows when a subscribe answer cannot be read", async () => {
    const { follower, last } = following({ "session.subscribe": () => Promise.resolve(null) });
    for (const event of replay.slice(0, 10)) follower.receive(event);
    follower.attached();
    await settle();
    expect(last().transcript.lastSeq).toBe(9);
    expect(last().stream).toEqual({ state: "live" });
  });

  it("goes offline when Core is lost even after subscribing failed", async () => {
    const { follower, last } = following({ "session.subscribe": () => Promise.reject({ kind: "unavailable", message: "busy" }) });
    follower.attached();
    await settle();
    expect(last().stream.state).toBe("failed");
    follower.detached();
    expect(last().stream).toEqual({ state: "offline" });
  });

  it("goes live on a subscribe answer it cannot read, rather than waiting for ever", async () => {
    const { follower, last } = following({ "session.subscribe": () => Promise.resolve(null) });
    follower.attached();
    await settle();
    expect(last().stream).toEqual({ state: "live" });
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

describe("stop", () => {
  // verifies: FLOW-SEND-MESSAGE-R03
  it("cancels a message still on its way: the send answers false, with no refusal shown", async () => {
    let refuse: (error: BridgeError) => void = () => {};
    const { follower, calls, last } = following({
      "session.send": () => new Promise((_, reject) => (refuse = reject)),
      "session.interrupt": () => Promise.resolve({}),
    });
    const sent = follower.send("hello");
    await settle();
    expect(last().message).toEqual({ state: "sending" });
    await follower.interrupt();
    expect(calls.at(-1)).toEqual({ method: "session.interrupt", params: { id: "s-replay" } });
    // Core answers the canceled send with an error.
    refuse({ kind: "rpc", error: { code: -32000, message: "the message was stopped before it was sent", data: { kind: "invalid_state" } } } as never);
    expect(await sent).toBe(false);
    expect(last().message).toBeUndefined();
  });

  // verifies: FLOW-SEND-MESSAGE-R03
  it("interrupts the turn when no message is on its way, and a later refusal still shows", async () => {
    const { follower, calls, last } = following({
      "session.interrupt": () => Promise.resolve({}),
      "session.send": () => Promise.reject(replayRefusal("session.send")),
    });
    await follower.interrupt();
    expect(calls.at(-1)).toEqual({ method: "session.interrupt", params: { id: "s-replay" } });
    expect(await follower.send("hello")).toBe(false);
    expect(last().message).toEqual({ state: "failed", message: "the session is a replay of a recording and cannot change" });
  });
});

describe("a stop that did not cancel the message", () => {
  // verifies: FLOW-SEND-MESSAGE-R05
  it("still shows the send's refusal when the interrupt never reached Core, or the send failed for another reason", async () => {
    for (const interrupt of [() => Promise.reject({ kind: "unavailable", message: "gone" }), () => Promise.resolve({})]) {
      let refuse: (error: BridgeError) => void = () => {};
      const { follower, last } = following({
        "session.send": () => new Promise((_, reject) => (refuse = reject)),
        "session.interrupt": interrupt,
      });
      const sent = follower.send("hello");
      await settle();
      await follower.interrupt();
      refuse({ kind: "rpc", error: { code: -32000, message: "the backend could not be reached", data: { kind: "internal" } } } as never);
      expect(await sent).toBe(false);
      expect(last().message).toEqual({ state: "failed", message: "the backend could not be reached" });
    }
  });
});

describe("a stop that never reached Core", () => {
  // verifies: FLOW-SEND-MESSAGE-R05
  it("leaves the send's refusal to show, even one Core gives as an invalid state", async () => {
    let refuse: (error: BridgeError) => void = () => {};
    const { follower, last } = following({
      "session.send": () => new Promise((_, reject) => (refuse = reject)),
      "session.interrupt": () => Promise.reject({ kind: "unavailable", message: "gone" }),
    });
    const sent = follower.send("hello");
    await settle();
    await follower.interrupt();
    refuse(replayRefusal("session.send"));
    expect(await sent).toBe(false);
    expect(last().message).toEqual({ state: "failed", message: "the session is a replay of a recording and cannot change" });
  });
});

describe("a turn Core has not shown yet", () => {
  const turnStarted = (seq: number) =>
    ({ v: EVENT_VERSION, session_id: "s-replay", seq, ts: "2026-01-01T00:00:00Z", source: { input: "backend" }, event: { type: "turn.started", turn: 1, trigger: "user" } }) as never;

  // verifies: FLOW-SEND-MESSAGE-R03
  it("keeps stop offered after Core accepts the message until the turn shows", async () => {
    const { follower, last } = following({ "session.send": () => Promise.resolve({ turn: 1 }) });
    expect(await follower.send("hello")).toBe(true);
    expect(last().turnPending).toBe(true);
    follower.receive(turnStarted(0));
    expect(last().turnPending).toBeUndefined();
    expect(last().transcript.working).toBe(true);
  });

  // verifies: FLOW-SEND-MESSAGE-R03
  it("leaves nothing pending when the turn showed before Core answered", async () => {
    let accept: (answer: unknown) => void = () => {};
    const { follower, last } = following({ "session.send": () => new Promise((r) => (accept = r)) });
    const sent = follower.send("hello");
    await settle();
    follower.receive(turnStarted(0));
    accept({ turn: 1 });
    expect(await sent).toBe(true);
    expect(last().turnPending).toBeUndefined();
  });
});
