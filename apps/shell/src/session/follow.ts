// Following one session: subscribe to its events, apply them to its
// transcript, subscribe again from where it left off whenever the window
// attaches anew, and send the user's messages (answers are the window's, not
// one follower's: answers.ts). What the window shows of the session comes
// only from the events Core projects; a message or an answer appears once
// Core records it, never before.

import type { Envelope } from "../protocol/agent-events.gen";
import { hasSeq } from "../core/checks";
import { type CoreClient, describeError } from "../core/client";
import { type Transcript, apply, emptyTranscript } from "./transcript";

/** The key that starts the notice saying Core no longer holds a session's earliest events. */
export const EARLIER = "earlier:";

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
  stream:
    | { state: "subscribing" }
    | { state: "live" }
    | { state: "offline" }
    /** `gap`: an event never arrived; the stream is live again if it still does. */
    | { state: "failed"; message: string; gap?: true };
  /** The composer's message, while it is on its way or after it was refused. */
  message?: Sending;
}

export interface Follower {
  /** An event Core sent for this session. */
  receive(envelope: Envelope): void;
  /** The window has a new connection to Core: subscribe on it. */
  attached(): void;
  /** The window lost its connection to Core. */
  detached(): void;
  /** Sends the user's message. Resolves true once Core accepted it; false when it was refused or stopped. */
  send(text: string): Promise<boolean>;
  /**
   * Stop (FLOW-SEND-MESSAGE-R03): cancels a message not yet handed to the agent, which stays with the user and is not
   * refused, or else interrupts the turn the agent is working on.
   */
  interrupt(): Promise<void>;
  /** Stops following; `update` is never called again. */
  stop(): void;
}

export function follow(client: CoreClient, id: string, update: (view: SessionView) => void): Follower {
  let view: SessionView = { id, transcript: emptyTranscript, stream: { state: "subscribing" } };
  let stopped = false;
  // Each subscribe gets a number; only the latest one's answer counts.
  let generation = 0;
  // The last event before a gap this follower already resubscribed for, so
  // one gap costs one resubscribe, not one per event behind it.
  let resumedAt: number | undefined;
  // Stop was chosen while a message was on its way: its failure is the stop's doing, not a refusal.
  let stopping = false;

  const set = (change: (view: SessionView) => SessionView) => {
    if (stopped) return;
    view = change(view);
    update(view);
  };

  update(view);

  // The view (one projection of the session, as Core names it) the events
  // shown came from; unknown until the first subscribe answers.
  let shownView: string | undefined;

  /** Drops what is shown, to apply events again from `first` on. */
  const startOver = (first: number) => {
    const transcript = { ...emptyTranscript, lastSeq: first - 1 };
    const items =
      first > 0
        ? [{ kind: "notice" as const, key: `${EARLIER}${first}`, message: "Earlier events of this session are no longer held." }]
        : [];
    set((v) => ({ ...v, transcript: { ...transcript, items } }));
    resumedAt = undefined;
  };

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
        const fields = readable ? (result as { last_seq?: unknown; view?: unknown; first_seq?: unknown }) : {};
        const raw = fields.last_seq;
        const last = !readable
          ? from - 1
          : raw === undefined
            ? -1
            : typeof raw === "number" && Number.isInteger(raw) && raw >= 0
              ? raw
              : from - 1;
        const isSeq = (n: unknown): n is number => typeof n === "number" && Number.isInteger(n) && n >= 0;
        const first = isSeq(fields.first_seq) ? fields.first_seq : 0;
        const answered = typeof fields.view === "string" && fields.view !== "" ? fields.view : undefined;
        // Another view (the session was opened afresh, or Core restarted):
        // what is shown came from a different projection and does not line
        // up with this one's numbers. Start over from what this view holds.
        if (answered !== undefined && shownView !== undefined && answered !== shownView && from > 0) {
          shownView = answered;
          startOver(first);
          attached();
          return;
        }
        if (answered !== undefined) shownView = answered;
        if (last < from - 1) {
          set((v) => ({ ...v, transcript: emptyTranscript }));
          resumedAt = undefined;
          attached();
          return;
        }
        // The view no longer holds what came between: what is shown is
        // complete only up to a gap, so it starts again where the view does.
        if (first > from) startOver(first);
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
          set((v) => ({ ...v, stream: { state: "failed", message: `event ${lastSeq + 1} never arrived`, gap: true } }));
        }
        return;
      }
      set((v) => {
        const transcript = apply(v.transcript, envelope);
        if (transcript === v.transcript) return v;
        // The missing event arrived after all: the stream is live again.
        const stream = v.stream.state === "failed" && v.stream.gap ? { state: "live" as const } : v.stream;
        return { ...v, transcript, stream };
      });
    },

    attached,

    detached() {
      ++generation; // an answer from the lost connection's subscribe no longer counts
      set((v) => ({ ...v, stream: { state: "offline" } }));
    },

    async send(text) {
      stopping = false;
      set((v) => ({ ...v, message: { state: "sending" } }));
      try {
        await client.call("session.send", { id, text });
        set((v) => ({ ...v, message: undefined }));
        return true;
      } catch (error) {
        const stopped = stopping;
        set((v) => ({ ...v, message: stopped ? undefined : { state: "failed", message: describeError(error) } }));
        return false;
      } finally {
        stopping = false;
      }
    },

    async interrupt() {
      if (view.message?.state === "sending") stopping = true;
      // Best effort: a turn that ended meanwhile, or a lost connection, leaves nothing to stop.
      await client.call("session.interrupt", { id }).catch(() => {});
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
