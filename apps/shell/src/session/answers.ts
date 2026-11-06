// The user's answers to permission requests (FLOW-APPROVE-PERMISSION, Sending an answer). An answer belongs to its
// session, not to whichever view sent it: one sent from the shelf is still on its way on the transcript's card when
// the user makes the session active meanwhile (R18). So the window keeps one record of answers on their way or
// refused, by session, and the shelf and the transcript both read it.

import type { Envelope } from "../protocol/agent-events.gen";
import { type CoreClient, type Params, describeError } from "../core/client";

/** What the user chose: Allow, Deny, or allowing with one of the request's suggestions, by its index. */
export type Decision = "allow" | "deny" | { suggestion: number };

/** An answer on its way, or refused with Core's reason; once its decision arrives as an event, it is gone. */
export type Answer = { state: "sending"; decision: Decision } | { state: "failed"; message: string };

/** Answers by session id, then by request id. */
export type Answers = Readonly<Record<string, Readonly<Record<string, Answer>>>>;

/** What Core is sent for a decision: no message or updated input (R13). */
function decideParams(id: string, requestId: string, decision: Decision): Params<"session.decide_permission"> {
  if (typeof decision === "object") return { id, request_id: requestId, behavior: "allow", apply_suggestions: [decision.suggestion] };
  return { id, request_id: requestId, behavior: decision };
}

export interface Answering {
  /** Sends one answer to a request, unless one is already on its way (R18). */
  decide(id: string, requestId: string, decision: Decision): void;
  /** An event Core sent: a decision settles the answer that was on its way. */
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

  return {
    decide(id, requestId, decision) {
      const key = `${id}\n${requestId}`;
      if (onItsWay.has(key)) return;
      onItsWay.add(key);
      set(id, requestId, { state: "sending", decision });
      client.call("session.decide_permission", decideParams(id, requestId, decision)).then(
        // Core took it; it stays on its way until its `permission.decided` arrives, so the answers stay unavailable.
        () => onItsWay.delete(key),
        (error) => {
          onItsWay.delete(key);
          // Refused: say why, and offer the answers again (R19).
          set(id, requestId, { state: "failed", message: describeError(error) }, true);
        },
      );
    },

    receive(envelope) {
      // Read defensively: the follower checks events, and this only looks for a decision's ids.
      const { session_id: id, event } = envelope as { session_id?: unknown; event?: { type?: unknown; request_id?: unknown } };
      if (typeof id === "string" && event?.type === "permission.decided" && typeof event.request_id === "string") {
        set(id, event.request_id, undefined);
      }
    },
  };
}
