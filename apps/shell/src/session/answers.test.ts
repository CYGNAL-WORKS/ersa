import { describe, expect, it } from "vitest";
import type { BridgeError, CoreClient, Method } from "../core/client";
import type { Envelope } from "../protocol/agent-events.gen";
import { type Answers, answering } from "./answers";

const settle = () => new Promise((r) => setTimeout(r, 0));

/** A client that records each call and answers decide_permission with `answer`; unanswered calls never settle. */
function setup(answer: () => Promise<unknown> = () => new Promise(() => {})) {
  const calls: { method: Method; params: unknown }[] = [];
  const client: CoreClient = {
    attach: () => new Promise(() => {}),
    call: (method, params) => {
      calls.push({ method, params });
      return answer() as never;
    },
  };
  let answers: Answers = {};
  const sending = answering(client, (update) => {
    answers = update(answers);
  });
  return { sending, calls, answers: () => answers };
}

const decided = (session_id: string, request_id: string) =>
  ({
    session_id,
    seq: 9,
    event: { type: "permission.decided", request_id, behavior: "allow", decided_by: "user" },
  }) as unknown as Envelope;

const refusal: BridgeError = {
  kind: "rpc",
  error: { code: -32000, message: "session.decide_permission: no pending request r1", data: { kind: "invalid_state" } },
};

describe("answering a permission request", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R13
  it("sends allow, deny, or allow with the chosen suggestion's index, and no message or updated input", () => {
    const { sending, calls } = setup();
    sending.decide("s", "r1", "allow");
    sending.decide("s", "r2", "deny");
    sending.decide("s", "r3", { suggestion: 2 });
    expect(calls).toEqual([
      { method: "session.decide_permission", params: { id: "s", request_id: "r1", behavior: "allow" } },
      { method: "session.decide_permission", params: { id: "s", request_id: "r2", behavior: "deny" } },
      { method: "session.decide_permission", params: { id: "s", request_id: "r3", behavior: "allow", apply_suggestions: [2] } },
    ]);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R18
  it("sends one answer, which stays on its way for its session until its decision arrives", async () => {
    const { sending, calls, answers } = setup(() => Promise.resolve({}));
    sending.decide("s", "r1", "deny");
    // A second choice while the first is on its way sends nothing.
    sending.decide("s", "r1", "allow");
    expect(calls).toHaveLength(1);
    expect(answers()).toEqual({ s: { r1: { state: "sending", decision: "deny" } } });
    await settle();
    // Core took it, but nothing is decided until Core says so.
    expect(answers()).toEqual({ s: { r1: { state: "sending", decision: "deny" } } });
    sending.receive(decided("other", "r1"));
    expect(answers()).toEqual({ s: { r1: { state: "sending", decision: "deny" } } });
    sending.receive(decided("s", "r1"));
    expect(answers()).toEqual({});
  });

  // verifies: FLOW-APPROVE-PERMISSION-R19
  it("keeps why Core refused an answer, in the user's terms, and takes another", async () => {
    let refuse = true;
    const { sending, calls, answers } = setup(() => (refuse ? Promise.reject(refusal) : new Promise(() => {})));
    sending.decide("s", "r1", "allow");
    await settle();
    // The method name Core puts first means nothing to the user and is dropped.
    expect(answers()).toEqual({ s: { r1: { state: "failed", message: "no pending request r1" } } });
    refuse = false;
    sending.decide("s", "r1", "deny");
    expect(calls).toHaveLength(2);
    expect(answers()).toEqual({ s: { r1: { state: "sending", decision: "deny" } } });
  });

  it("keeps nothing of a refusal that comes after the decision arrived", async () => {
    let refuse: (error: unknown) => void = () => {};
    const { sending, answers } = setup(() => new Promise((_, reject) => (refuse = reject)));
    sending.decide("s", "r1", "allow");
    sending.receive(decided("s", "r1"));
    refuse(refusal);
    await settle();
    expect(answers()).toEqual({});
  });
});

const questionAnswered = (session_id: string, request_id: string, answered_by: "user" | "backend") =>
  ({
    session_id,
    seq: 9,
    event: { type: "question.answered", request_id, answers: [], declined: false, answered_by },
  }) as unknown as Envelope;

describe("answering a question", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R16
  it("sends one answer per question, own words included, and no declined", () => {
    const { sending, calls } = setup();
    const answers = [
      { id: "q1", value: "o2", own_words: false },
      { id: "q2", value: ["o1", "o3"], own_words: false },
      { id: "q3", value: "retry-on-windows", own_words: true },
    ];
    sending.answer("s", "r1", { answers });
    expect(calls).toEqual([{ method: "session.answer_question", params: { id: "s", request_id: "r1", answers } }]);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R28
  it("sends declined with no answers when the user declines", () => {
    const { sending, calls } = setup();
    sending.answer("s", "r1", "decline");
    expect(calls).toEqual([{ method: "session.answer_question", params: { id: "s", request_id: "r1", answers: [], declined: true } }]);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R18
  it("sends one answer, which stays on its way until the question's answer arrives, whoever answered it", async () => {
    const { sending, calls, answers } = setup(() => Promise.resolve({}));
    sending.answer("s", "r1", "decline");
    sending.answer("s", "r1", { answers: [] });
    expect(calls).toHaveLength(1);
    await settle();
    expect(answers()).toEqual({ s: { r1: { state: "sending", decision: "decline" } } });
    sending.receive(questionAnswered("s", "r1", "backend"));
    expect(answers()).toEqual({});
  });
});
