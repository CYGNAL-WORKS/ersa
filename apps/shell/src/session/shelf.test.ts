import { describe, expect, it } from "vitest";
import type { CoreClient, Method, Params } from "../core/client";
import type { Envelope, Event } from "../protocol/agent-events.gen";
import type { Session } from "../protocol/core-api.gen";
import type { SessionView } from "./follow";
import { shelfAfter, shelfFollowing, shelfOf, shelfTone } from "./shelf";
import { EVENT_VERSION, apply, emptyTranscript } from "./transcript";

const flags = { default_to_no: false, suppress_always_allow: false, requires_user_interaction: false, classifier_approvable: false };
const permission = (request_id: string, more: Partial<Extract<Event, { type: "permission.requested" }>> = {}): Event => ({
  type: "permission.requested",
  request_id,
  tool_use_id: `t-${request_id}`,
  tool_name: "Bash",
  input: { command: `echo ${request_id}` },
  suggestions: [],
  flags,
  ...more,
});
const question = (request_id: string): Event => ({
  type: "question.asked",
  request_id,
  questions: [{ id: "q1", text: "Which branch?", multi_select: false }],
});

const envelope = (event: Event, seq: number, session_id = "s1"): Envelope => ({
  v: EVENT_VERSION,
  session_id,
  seq,
  ts: "2026-11-06T09:00:00Z",
  source: { backend: "claude-code", backend_version: "2.1.302", input: "live" },
  event,
});

const viewOf = (events: Event[], more: Partial<SessionView> = {}): SessionView => ({
  id: "s1",
  transcript: events.map((e, seq) => envelope(e, seq)).reduce(apply, emptyTranscript),
  stream: { state: "live" },
  answers: {},
  ...more,
});

type Kind = Session["tile"]["pending"][number]["kind"];
const pending = (...ids: [Kind, string][]) => ids.map(([kind, request_id]) => ({ kind, request_id }));

const session = (id: string, waiting: [Kind, string][] = []): Session => ({
  id,
  workspace_id: "w",
  host_id: "local",
  backend: "claude-code",
  model: "opus",
  effort: "medium",
  permission_mode: "default",
  state: "waiting_user",
  created_at: "2026-11-06T09:00:00Z",
  tile: { state: "waiting_user", pending: pending(...waiting) },
});

describe("which shelf is open", () => {
  const two = [session("a", [["permission", "r1"]]), session("b", [["permission", "r2"]]), session("active", [["permission", "r3"]])];

  // That the active session stays as it was is checked in the running window (live/shelf.live.ts).
  // verifies: FLOW-APPROVE-PERMISSION-R01
  it("opens the chosen tile's shelf", () => {
    expect(shelfAfter(undefined, { type: "chevron", id: "a", activeId: "active" })).toBe("a");
  });

  // verifies: FLOW-APPROVE-PERMISSION-R02
  it("never opens a shelf when a request arrives", () => {
    const arrived = [session("a", [["permission", "r1"], ["question", "r9"]]), session("b", [["permission", "r2"]])];
    expect(shelfAfter(undefined, { type: "sessions", sessions: arrived, activeId: "active" })).toBeUndefined();
    // One already open stays the one open.
    expect(shelfAfter("a", { type: "sessions", sessions: arrived, activeId: "active" })).toBe("a");
  });

  // verifies: FLOW-APPROVE-PERMISSION-R03
  it("closes the open shelf when another tile's is opened, so at most one is open", () => {
    const open = shelfAfter(undefined, { type: "chevron", id: "a", activeId: "active" });
    expect(shelfAfter(open, { type: "chevron", id: "b", activeId: "active" })).toBe("b");
  });

  // verifies: FLOW-APPROVE-PERMISSION-R26
  it("opens no shelf from the active session's chevron, and closes a shelf whose session is made active", () => {
    expect(shelfAfter(undefined, { type: "chevron", id: "active", activeId: "active" })).toBeUndefined();
    expect(shelfAfter("a", { type: "chevron", id: "active", activeId: "active" })).toBe("a");
    expect(shelfAfter("a", { type: "sessions", sessions: two, activeId: "a" })).toBeUndefined();
  });

  it("closes when its session has nothing left to ask, or is gone", () => {
    expect(shelfAfter("a", { type: "sessions", sessions: [session("a")], activeId: "active" })).toBeUndefined();
    expect(shelfAfter("a", { type: "sessions", sessions: [], activeId: "active" })).toBeUndefined();
  });

  it("closes when its chevron is chosen again", () => {
    expect(shelfAfter("a", { type: "chevron", id: "a", activeId: "active" })).toBeUndefined();
  });
});

describe("what the shelf shows", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R01
  it("shows the oldest pending request, the one whose event came first, whatever order the tile lists them in", () => {
    const view = viewOf([permission("r1"), permission("r2"), question("r3")]);
    const shows = shelfOf(view, pending(["question", "r3"], ["permission", "r2"], ["permission", "r1"]));
    expect(shows).toMatchObject({ state: "permission", request: { request_id: "r1" }, place: { n: 1, m: 3 } });
    expect(shelfTone(shows)).toBe("caution");
  });

  it("skips requests already answered and the ones the tile no longer lists", () => {
    const view = viewOf([
      permission("r1"),
      { type: "permission.decided", request_id: "r1", behavior: "allow", decided_by: "user" },
      permission("r2"),
      question("r3"),
    ]);
    expect(shelfOf(view, pending(["question", "r3"]))).toMatchObject({ state: "question", requestId: "r3" });
    expect(shelfOf(view, pending(["question", "r3"]))).not.toHaveProperty("place.m");
    expect(shelfTone(shelfOf(view, pending(["question", "r3"])))).toBe("accent");
  });

  // verifies: FLOW-APPROVE-PERMISSION-R11
  it("shows a permission request's own event, with everything the shelf reads from it", () => {
    const request = permission("r1", {
      display_name: "Run a command",
      description: "Run the reconnect tests",
      blocked_path: "C:\\Users\\me\\ersa\\target",
      reason: { kind: "rule", text: "Commands need approval in this project." },
    });
    const shows = shelfOf(viewOf([request]), pending(["permission", "r1"]));
    expect(shows).toEqual({ state: "permission", request: { ...request }, place: undefined });
  });

  // verifies: FLOW-APPROVE-PERMISSION-R21
  it("is loading while the events are read, and says why reading them failed", () => {
    expect(shelfOf(undefined, pending(["permission", "r1"]))).toEqual({ state: "loading" });
    expect(shelfOf(viewOf([], { stream: { state: "subscribing" } }), pending(["permission", "r1"]))).toEqual({ state: "loading" });
    expect(shelfOf(viewOf([], { stream: { state: "failed", message: "the session is gone" } }), pending(["permission", "r1"]))).toEqual({
      state: "failed",
      message: "the session is gone",
    });
  });

  // verifies: FLOW-APPROVE-PERMISSION-R21
  it("says reading failed when a lost event did not arrive on subscribing again, and is loading while it does", () => {
    const failed = viewOf([permission("r1")], { stream: { state: "failed", message: "event 1 never arrived", gap: true } });
    expect(shelfOf(failed, pending(["permission", "r1"]))).toEqual({ state: "failed", message: "event 1 never arrived" });
    expect(shelfOf(viewOf([permission("r1")], { stream: { state: "subscribing" } }), pending(["permission", "r1"]))).toEqual({ state: "loading" });
  });

  // verifies: FLOW-APPROVE-PERMISSION-R25
  it("says a request can't be shown when its kind is unknown or Core no longer holds its event", () => {
    // A kind a newer Core may report, which this window doesn't know.
    const unknownKind = [{ kind: "approval" as Kind, request_id: "r7" }];
    expect(shelfOf(viewOf([]), unknownKind)).toEqual({ state: "unknown", place: undefined });
    // Its event is not among those held: Core let go of the earliest events, where it was.
    const afterGap: SessionView = {
      ...viewOf([]),
      transcript: { lastSeq: 40, items: [{ kind: "notice", key: "earlier:30", message: "Earlier events of this session are no longer held." }] },
    };
    const late = apply(afterGap.transcript, envelope(permission("r2"), 41));
    expect(shelfOf({ ...afterGap, transcript: late }, pending(["permission", "r1"], ["permission", "r2"]))).toEqual({
      state: "unknown",
      place: { n: 1, m: 2 },
    });
    // With nothing held about it at all, it can't be shown either.
    expect(shelfOf(viewOf([permission("r2")]), pending(["permission", "r1"]))).toEqual({ state: "unknown", place: undefined });
  });
});

/** A client that records each call; subscribing answers at once with no events. */
function fakeClient() {
  const calls: { method: Method; params: unknown }[] = [];
  const client: CoreClient = {
    attach: () => new Promise(() => {}),
    call: <M extends Method>(method: M, params?: Params<M>) => {
      calls.push({ method, params });
      return Promise.resolve(method === "session.subscribe" ? {} : null) as never;
    },
  };
  return { client, calls };
}

const subscriptions = (calls: { method: Method; params: unknown }[]) => {
  const open = new Set<string>();
  for (const { method, params } of calls) {
    const { id } = params as { id: string };
    if (method === "session.subscribe") open.add(id);
    if (method === "session.unsubscribe") open.delete(id);
  }
  return open;
};

describe("following the shelf's session", () => {
  it("follows the open shelf's session, moves with the shelf, and leaves no subscription once it closes", () => {
    const { client, calls } = fakeClient();
    const seen: (SessionView | undefined)[] = [];
    const shelf = shelfFollowing(client, (v) => seen.push(v));
    shelf.follow("a", true);
    expect(subscriptions(calls)).toEqual(new Set(["a"]));
    shelf.follow("b", true);
    expect(subscriptions(calls)).toEqual(new Set(["b"]));
    shelf.follow(undefined, true);
    expect(subscriptions(calls)).toEqual(new Set());
    // Only the shelf's own sessions were ever asked for: the active session's subscription is the transcript's.
    expect(new Set(calls.map((c) => (c.params as { id: string }).id))).toEqual(new Set(["a", "b"]));
    expect(seen.at(-1)).toBeUndefined();
  });

  it("subscribes once the window is connected, and applies only its own session's events", () => {
    const { client, calls } = fakeClient();
    let last: SessionView | undefined;
    const shelf = shelfFollowing(client, (v) => (last = v));
    shelf.follow("s1", false);
    expect(calls).toEqual([]);
    shelf.attached();
    expect(subscriptions(calls)).toEqual(new Set(["s1"]));
    shelf.receive(envelope(permission("r1"), 0));
    shelf.receive(envelope(permission("x1"), 1, "other"));
    expect(last?.transcript.items.map((i) => i.key)).toEqual(["permission:r1"]);
  });

  it("leaves the subscription alone when the shelf closes because its session became active", () => {
    const { client, calls } = fakeClient();
    const shelf = shelfFollowing(client, () => {});
    shelf.follow("a", true, "other");
    // The transcript subscribes to "a" as it becomes active; the shelf closing must not end that subscription.
    shelf.follow(undefined, true, "a");
    expect(calls.filter((c) => c.method === "session.unsubscribe")).toEqual([]);
    // Closed for any other reason, it unsubscribes.
    shelf.follow("b", true, "a");
    shelf.follow(undefined, true, "a");
    expect(calls.filter((c) => c.method === "session.unsubscribe").map((c) => c.params)).toEqual([{ id: "b" }]);
  });

  it("subscribes again on Retry", () => {
    const { client, calls } = fakeClient();
    const shelf = shelfFollowing(client, () => {});
    shelf.follow("a", true);
    shelf.attached();
    expect(calls.filter((c) => c.method === "session.subscribe")).toHaveLength(2);
  });
});
