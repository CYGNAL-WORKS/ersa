// The user's answers to permission requests and questions (FLOW-APPROVE-PERMISSION, Sending an answer). An answer belongs to its
// session, not to whichever view sent it: one sent from the shelf is still on its way on the transcript's card when
// the user makes the session active meanwhile (R18). So the window keeps one record of answers on their way or
// refused, by session, and the shelf and the transcript both read it.

import type { Envelope, QuestionAnswered } from "../protocol/agent-events.gen";
import { type CoreClient, type Params, describeError } from "../core/client";

/** What the user chose: Allow, Deny, or allowing with one of the request's suggestions, by its index. */
export type Decision = "allow" | "deny" | { suggestion: number };

/** What the user gave a question: an answer to every question in it (R16), or Decline (R28). */
export type Reply = { answers: QuestionAnswered["answers"] } | "decline";

/** An answer on its way, or refused with Core's reason; once its decision or answer arrives as an event, it is gone. */
export type Answer = { state: "sending"; decision: Decision | Reply } | { state: "failed"; message: string };

/** Answers by session id, then by request id. */
export type Answers = Readonly<Record<string, Readonly<Record<string, Answer>>>>;

/** What Core is sent for a decision: no message or updated input (R13). */
function decideParams(id: string, requestId: string, decision: Decision): Params<"session.decide_permission"> {
  if (typeof decision === "object") return { id, request_id: requestId, behavior: "allow", apply_suggestions: [decision.suggestion] };
  return { id, request_id: requestId, behavior: decision };
}

/** What Core is sent for a question: its answers, or `declined` with none (R16, R28). */
function answerParams(id: string, requestId: string, reply: Reply): Params<"session.answer_question"> {
  if (reply === "decline") return { id, request_id: requestId, answers: [], declined: true };
  return { id, request_id: requestId, answers: reply.answers };
}

export interface Answering {
  /** Sends one answer to a request, unless one is already on its way (R18). */
  decide(id: string, requestId: string, decision: Decision): void;
  /** Answers a question, unless an answer to it is already on its way (R18). */
  answer(id: string, requestId: string, reply: Reply): void;
  /** An event Core sent: a decision or a question's answer settles the answer that was on its way. */
  receive(envelope: Envelope): void;
}

export function answering(client: CoreClient, change: (update: (answers: Answers) => Answers) => void): Answering {
  // What is on its way, kept here as well as in the window's state, so a second choice before React renders the
  // first is still refused.
  const onItsWay = new Set<string>();
  // `onlyHeld`: change the answer only while one is recorded, so a refusal that comes after the decision arrived
  // leaves nothing behind.
  const set = (id: string, requestId: string, answer: Answer | undefined, onlyHeld = false) =>
    change((answers) => {
      // Nothing to settle: the same record, so nothing renders again.
      if ((!answer || onlyHeld) && !answers[id]?.[requestId]) return answers;
      const session = { ...answers[id] };
      if (answer) session[requestId] = answer;
      else delete session[requestId];
      const next = { ...answers, [id]: session };
      if (Object.keys(session).length === 0) delete next[id];
      return next;
    });

  // One answer at a time to a request, kept on its way until Core's event for it arrives.
  const send = (id: string, requestId: string, decision: Decision | Reply, call: () => Promise<unknown>) => {
    const key = `${id}\n${requestId}`;
    if (onItsWay.has(key)) return;
    onItsWay.add(key);
    set(id, requestId, { state: "sending", decision });
    call().then(
      // Core took it; it stays on its way until its event arrives, so the answers stay unavailable.
      () => onItsWay.delete(key),
      (error) => {
        onItsWay.delete(key);
        // Refused: say why, and offer the answers again (R19).
        set(id, requestId, { state: "failed", message: describeError(error) }, true);
      },
    );
  };

  return {
    decide(id, requestId, decision) {
      send(id, requestId, decision, () => client.call("session.decide_permission", decideParams(id, requestId, decision)));
    },

    answer(id, requestId, reply) {
      send(id, requestId, reply, () => client.call("session.answer_question", answerParams(id, requestId, reply)));
    },

    receive(envelope) {
      // Read defensively: the follower checks events, and this only looks for a decision's or an answer's ids.
      const { session_id: id, event } = envelope as { session_id?: unknown; event?: { type?: unknown; request_id?: unknown } };
      const settles = event?.type === "permission.decided" || event?.type === "question.answered";
      if (typeof id === "string" && settles && typeof event.request_id === "string") {
        set(id, event.request_id, undefined);
      }
    },
  };
}
