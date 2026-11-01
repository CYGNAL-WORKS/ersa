// Following one session: subscribe to its events, apply them to its
// transcript, subscribe again from where it left off whenever the window
// attaches anew, and send the user's messages and answers. What the window
// shows of the session comes only from the events Core projects; a message
// or an answer appears once Core records it, never before.

import type { Envelope } from "../protocol/agent-events.gen";
import { hasSeq } from "../core/checks";
import { type CoreClient, describeError } from "../core/client";
import { type Transcript, apply, emptyTranscript } from "./transcript";

/** The state of something the user sent: on its way, or refused with a reason. */
export type Sending = { state: "sending" } | { state: "failed"; message: string };

export interface SessionView {
  id: string;
  transcript: Transcript;
  /**
   * Whether the events are flowing: subscribing, live, offline (the window
   * has no connection to Core; answers wait until it attaches again), or why
   * subscribing failed.
   */
  stream: { state: "subscribing" } | { state: "live" } | { state: "offline" } | { state: "failed"; message: string };
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
  /** The window lost its connection to Core. */
  detached(): void;
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
  // The last event before a gap this follower already resubscribed for, so
  // one gap costs one resubscribe, not one per event behind it.
  let resumedAt: number | undefined;

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

  /** Subscribes on the current connection from after the last event applied. */
  const attached = () => {
    if (stopped) return;
    const mine = ++generation;
    set((v) => ({ ...v, stream: { state: "subscribing" } }));
    // Every event after the last one applied; events Core sends again
    // (the subscription overlaps a notification already received) are
    // ignored by seq.
    const from = view.transcript.lastSeq + 1;
    client.call("session.subscribe", { id, from_seq: from }).then(
      (result) => {
        if (stopped || mine !== generation) return;
        // Core's history is shorter than what is shown (a replay restarted
        // on the same endpoint, a reprojected session): what is shown is not
        // this session's any more. Start again from the beginning.
        // `last_seq` absent means the session has no events; an answer that
        // cannot be read (not an object, or a `last_seq` that is no
        // sequence number) is taken to mean nothing was lost.
        const readable = typeof result === "object" && result !== null;
        const raw = readable ? (result as { last_seq?: unknown }).last_seq : undefined;
        const last = !readable
          ? from - 1
          : raw === undefined
            ? -1
            : typeof raw === "number" && Number.isInteger(raw) && raw >= 0
              ? raw
              : from - 1;
        if (last < from - 1) {
          set((v) => ({ ...v, transcript: emptyTranscript, answers: {} }));
          resumedAt = undefined;
          attached();
          return;
        }
        set((v) => ({ ...v, stream: { state: "live" } }));
      },
      (error) => {
        if (mine === generation) set((v) => ({ ...v, stream: { state: "failed", message: describeError(error) } }));
      },
    );
  };

  return {
    receive(envelope) {
      if (!hasSeq(envelope) || (envelope as { session_id?: unknown }).session_id !== id) return;
      const { lastSeq } = view.transcript;
      // An event beyond the next on a live subscription means one was lost on
      // the way (Core's sequence is gap-free): subscribe again from the gap.
      // While subscribing, a stray from an earlier subscription is expected
      // and the backlog fills in behind it.
      if (envelope.seq > lastSeq + 1 && view.stream.state === "live") {
        if (resumedAt !== lastSeq) {
          resumedAt = lastSeq;
          attached();
        } else {
          // Subscribing again did not fill the gap: say so, so Retry shows,
          // rather than read "live" while events are dropped.
          set((v) => ({ ...v, stream: { state: "failed", message: `event ${lastSeq + 1} never arrived` } }));
        }
        return;
      }
      set((v) => {
        const transcript = apply(v.transcript, envelope);
        if (transcript === v.transcript) return v;
        // A decision that arrived settles the answer that was on its way.
        // `apply` has checked the event; one it could not read has no type here.
        const event = (envelope as { event?: unknown }).event as Envelope["event"] | undefined;
        const answers =
          event?.type === "permission.decided" && event.request_id in v.answers
            ? Object.fromEntries(Object.entries(v.answers).filter(([key]) => key !== event.request_id))
            : v.answers;
        return { ...v, transcript, answers };
      });
    },

    attached,

    detached() {
      ++generation; // an answer from the lost connection's subscribe no longer counts
      set((v) => ({ ...v, stream: { state: "offline" } }));
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
