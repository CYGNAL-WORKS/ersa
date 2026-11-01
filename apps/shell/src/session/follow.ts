// Following one session: subscribe to its events, apply them to its
// transcript, subscribe again from where it left off whenever the window
// attaches anew, and send the user's messages and answers. What the window
// shows of the session comes only from the events Core projects; a message
// or an answer appears once Core records it, never before.

import type { Envelope } from "../protocol/agent-events.gen";
import { type CoreClient, describeError } from "../core/client";
import { type Transcript, apply, emptyTranscript } from "./transcript";

/** The state of something the user sent: on its way, or refused with a reason. */
export type Sending = { state: "sending" } | { state: "failed"; message: string };

export interface SessionView {
  id: string;
  transcript: Transcript;
  /** Whether the events are flowing: subscribing, live, or why subscribing failed. */
  stream: { state: "subscribing" } | { state: "live" } | { state: "failed"; message: string };
  /** The composer's message, while it is on its way or after it was refused. */
  message?: Sending;
  /** Answers to permission requests, by request id, until the decision arrives as an event. */
  answers: Record<string, Sending>;
}

export interface Follower {
  /** An event Core sent for this session. */
  receive(envelope: Envelope): void;
  /** The window has a new connection to Core: subscribe on it. */
  attached(): void;
  /** Sends the user's message. Resolves true once Core accepted it. */
  send(text: string): Promise<boolean>;
  decide(requestId: string, behavior: "allow" | "deny"): void;
  /** Stops following; `update` is never called again. */
  stop(): void;
}

export function follow(client: CoreClient, id: string, update: (view: SessionView) => void): Follower {
  let view: SessionView = { id, transcript: emptyTranscript, stream: { state: "subscribing" }, answers: {} };
  let stopped = false;
  // Each subscribe gets a number; only the latest one's answer counts.
  let generation = 0;

  const set = (change: (view: SessionView) => SessionView) => {
    if (stopped) return;
    view = change(view);
    update(view);
  };

  const withAnswer = (requestId: string, answer: Sending | undefined) =>
    set((v) => {
      const answers = { ...v.answers };
      if (answer) answers[requestId] = answer;
      else delete answers[requestId];
      return { ...v, answers };
    });

  update(view);

  return {
    receive(envelope) {
      if (envelope.session_id !== id) return;
      set((v) => {
        const transcript = apply(v.transcript, envelope);
        if (transcript === v.transcript) return v;
        // A decision that arrived settles the answer that was on its way.
        const event = envelope.event;
        const answers =
          event.type === "permission.decided" && event.request_id in v.answers
            ? Object.fromEntries(Object.entries(v.answers).filter(([key]) => key !== event.request_id))
            : v.answers;
        return { ...v, transcript, answers };
      });
    },

    attached() {
      const mine = ++generation;
      set((v) => ({ ...v, stream: { state: "subscribing" } }));
      // Every event after the last one applied; events Core sends again
      // (the subscription overlaps a notification already received) are
      // ignored by seq.
      client.call("session.subscribe", { id, from_seq: view.transcript.lastSeq + 1 }).then(
        () => {
          if (mine === generation) set((v) => ({ ...v, stream: { state: "live" } }));
        },
        (error) => {
          if (mine === generation) set((v) => ({ ...v, stream: { state: "failed", message: describeError(error) } }));
        },
      );
    },

    async send(text) {
      set((v) => ({ ...v, message: { state: "sending" } }));
      try {
        await client.call("session.send", { id, text });
        set((v) => ({ ...v, message: undefined }));
        return true;
      } catch (error) {
        set((v) => ({ ...v, message: { state: "failed", message: describeError(error) } }));
        return false;
      }
    },

    decide(requestId, behavior) {
      withAnswer(requestId, { state: "sending" });
      client.call("session.decide_permission", { id, request_id: requestId, behavior }).then(
        // Core answered; the answer settles when its `permission.decided`
        // arrives. If that came first, there is nothing left to settle.
        () => {},
        (error) => withAnswer(requestId, { state: "failed", message: describeError(error) }),
      );
    },

    stop() {
      if (stopped) return;
      stopped = true;
      // Best effort: the connection may already be gone, which ends the
      // subscription anyway.
      client.call("session.unsubscribe", { id }).catch(() => {});
    },
  };
}
