// Following one session: subscribe to its events, apply them to its
// transcript, subscribe again from where it left off whenever the window
// attaches anew, and send the user's messages (answers are the window's, not
// one follower's: answers.ts). What the window shows of the session comes
// only from the events Core projects; a message or an answer appears once
// Core records it, never before.

import type { Envelope } from "../protocol/agent-events.gen";
import type { Attachment, SessionSendProgressNotification } from "../protocol/core-api.gen";
import { hasSeq } from "../core/checks";
import { type BridgeError, type CoreClient, describeError } from "../core/client";
import { type Transcript, apply, emptyTranscript } from "./transcript";

/** The key that starts the notice saying Core no longer holds a session's earliest events. */
export const EARLIER = "earlier:";

/** How far a message on its way has got (FLOW-SEND-MESSAGE-R16): its attachments' copy, as a share, or the backend starting. */
export type SendProgress = { phase: "copying"; share: number } | { phase: "starting" };

/**
 * The state of something the user sent: on its way, refused with a reason, or a message Ersa can't tell reached the
 * agent (FLOW-SEND-MESSAGE-R15). A message refused for one of its attachments names it by its place (R10).
 */
export type Sending =
  | { state: "sending"; progress?: SendProgress }
  | { state: "failed"; message: string; index?: number }
  | { state: "unknown" };

/** What `session.send_progress` says to show; nothing for checking or handing over, which are quick. */
export function progressOf(notification: SessionSendProgressNotification): SendProgress | undefined {
  if (notification.phase === "starting") return { phase: "starting" };
  if (notification.phase !== "copying") return undefined;
  const { copied_bytes: copied = 0, total_bytes: total = 0 } = notification;
  return { phase: "copying", share: total > 0 ? Math.min(Math.max(copied / total, 0), 1) : 0 };
}

/**
 * What a failed `session.send` means for the message: Core handed it over and can't tell (`delivery_unknown`), or
 * the send reached Core and then lost its connection or got no answer, so Ersa can't tell either
 * (FLOW-SEND-MESSAGE-R15); else not sent, with why, naming the attachment refused (R10). A send that never reached
 * Core, as one too large to write, was not sent.
 */
export function sendFailure(error: unknown): Exclude<Sending, { state: "sending" }> {
  const bridge = error as BridgeError | undefined;
  if (bridge?.kind === "unavailable" && bridge.sent) return { state: "unknown" };
  const data = bridge?.kind === "rpc" ? bridge.error.data : undefined;
  if (data?.kind === "delivery_unknown") return { state: "unknown" };
  const index = data?.kind === "attachment_refused" && Number.isInteger(data.index) ? data.index : undefined;
  return index === undefined ? { state: "failed", message: describeError(error) } : { state: "failed", message: describeError(error), index };
}

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
  /**
   * Core accepted the message, and its turn has not shown on the subscription yet: Core can answer the send before
   * the turn's events reach the window, and stop stays offered meanwhile (FLOW-SEND-MESSAGE-R03, R04).
   */
  turnPending?: boolean;
}

export interface Follower {
  /** An event Core sent for this session. */
  receive(envelope: Envelope): void;
  /** The window has a new connection to Core: subscribe on it. */
  attached(): void;
  /** The window lost its connection to Core. */
  detached(): void;
  /** Sends the user's message. Resolves true once Core accepted it; false when it was refused, stopped, or may not have arrived. */
  send(text: string, attachments?: Attachment[]): Promise<boolean>;
  /** Where Core says this session's send is (FLOW-SEND-MESSAGE-R16). */
  progress(notification: SessionSendProgressNotification): void;
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
  // A turn showed on the subscription since the latest send began, so its answer needs no pending turn.
  let turnSeen = false;

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
        // The turn the send started has shown: the transcript says whether it still works.
        const type = envelope.event?.type;
        const turned = type === "turn.started" || type === "turn.completed" || type === "session.ended";
        if (turned) turnSeen = true;
        return { ...v, transcript, stream, turnPending: turned ? undefined : v.turnPending };
      });
    },

    attached,

    detached() {
      ++generation; // an answer from the lost connection's subscribe no longer counts
      set((v) => ({ ...v, stream: { state: "offline" } }));
    },

    async send(text, attachments) {
      stopping = false;
      turnSeen = false;
      set((v) => ({ ...v, message: { state: "sending" } }));
      try {
        await client.call("session.send", attachments?.length ? { id, text, attachments } : { id, text });
        const pending = !turnSeen;
        set((v) => ({ ...v, message: undefined, turnPending: pending || undefined }));
        return true;
      } catch (error) {
        // Only Core's cancel, answered as an invalid state, is the stop's doing; any other failure is still shown.
        const stopped = stopping && (error as BridgeError | undefined)?.kind === "rpc" && (error as Extract<BridgeError, { kind: "rpc" }>).error.data?.kind === "invalid_state";
        set((v) => ({ ...v, message: stopped ? undefined : sendFailure(error) }));
        return false;
      } finally {
        stopping = false;
      }
    },

    progress(notification) {
      if (notification.session_id !== id || view.message?.state !== "sending") return;
      const progress = progressOf(notification);
      set((v) => ({ ...v, message: { state: "sending", progress } }));
    },

    async interrupt() {
      if (view.message?.state === "sending") stopping = true;
      // Best effort: a turn that ended meanwhile, or a lost connection, leaves nothing to stop. A stop that never
      // reached Core stopped nothing, so a failure of the send after it is still a refusal to show.
      await client.call("session.interrupt", { id }).catch(() => {
        stopping = false;
      });
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
