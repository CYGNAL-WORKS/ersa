// Following one session: subscribe to its events, apply them to its
// transcript, subscribe again from where it left off whenever the window
// attaches anew, and send the user's messages (answers are the window's, not
// one follower's: answers.ts). What the window shows of the session comes
// only from the events Core projects; a message or an answer appears once
// Core records it, never before.

import type { Envelope } from "../protocol/agent-events.gen";
import type { Attachment, SessionSendProgressNotification } from "../protocol/core-api.gen";
import { hasSeq } from "../core/checks";
import { type BridgeError, type CoreClient, describeError, refusalOf, stoppedBeforeSent } from "../core/client";
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
  const data = refusalOf(error);
  if (data?.kind === "delivery_unknown") return { state: "unknown" };
  const index = data?.kind === "attachment_refused" && Number.isInteger(data.index) ? data.index : undefined;
  return index === undefined ? { state: "failed", message: describeError(error) } : { state: "failed", message: describeError(error), index };
}

/**
 * A message this window sent that the backend holds behind the running turn, by the name the status line gives it
 * (FLOW-SEND-MESSAGE-R19), with the last event shown when it was sent: the `user.message` that takes it in comes after.
 */
export interface WaitingHere {
  name: string;
  after: number;
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
   * the turn's events reach the window, and stop stays offered meanwhile (FLOW-SEND-MESSAGE-R03).
   */
  turnPending?: boolean;
  /** The messages this window sent that wait in the backend, oldest first (FLOW-SEND-MESSAGE-R19). */
  waitingHere?: readonly WaitingHere[];
  /** Stop was chosen, and the turn it stops has not ended: a second stop sends nothing (FLOW-SEND-MESSAGE-R20). */
  stopChosen?: true;
}

export interface Follower {
  /** An event Core sent for this session. */
  receive(envelope: Envelope): void;
  /** The window has a new connection to Core: subscribe on it. */
  attached(): void;
  /** The window lost its connection to Core. */
  detached(): void;
  /**
   * Sends the user's message, by `name` on the status line while it waits in the backend (FLOW-SEND-MESSAGE-R19).
   * Resolves true once Core accepted it; false when it was refused, stopped, or may not have arrived.
   */
  send(text: string, attachments?: Attachment[], name?: string): Promise<boolean>;
  /** Where Core says this session's send is (FLOW-SEND-MESSAGE-R16). */
  progress(notification: SessionSendProgressNotification): void;
  /**
   * Stop (FLOW-SEND-MESSAGE-R03): cancels a message not yet handed to the agent, which stays with the user and is not
   * refused, or else interrupts the turn the agent is working on. Once per turn: a second stop before that turn ends
   * would stop the next one, which a waiting message runs (R20).
   */
  interrupt(): Promise<void>;
  /** Stops following; `update` is never called again. */
  stop(): void;
}

/** `waitingHere`: what this window sent to the session that still waits, kept from following it before. */
export function follow(client: CoreClient, id: string, update: (view: SessionView) => void, waitingHere?: readonly WaitingHere[]): Follower {
  let view: SessionView = { id, transcript: emptyTranscript, stream: { state: "subscribing" }, ...(waitingHere?.length ? { waitingHere } : {}) };
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
        // The backend took in what waited (one user.message may carry several), or the session ended: every message
        // sent before this event no longer waits. Events replayed from before a send keep it.
        const taken = type === "user.message" || type === "session.ended";
        const waiting = taken ? v.waitingHere?.filter((w) => w.after >= envelope.seq) : v.waitingHere;
        return {
          ...v,
          transcript,
          stream,
          turnPending: turned ? undefined : v.turnPending,
          // A turn that began or ended takes a stop afresh.
          stopChosen: turned ? undefined : v.stopChosen,
          waitingHere: waiting?.length ? waiting : undefined,
        };
      });
    },

    attached,

    detached() {
      ++generation; // an answer from the lost connection's subscribe no longer counts
      set((v) => ({ ...v, stream: { state: "offline" } }));
    },

    async send(text, attachments, name) {
      stopping = false;
      turnSeen = false;
      const after = view.transcript.lastSeq;
      // A stop chosen before belongs to the turn; this send can be stopped on its own (R03).
      set((v) => ({ ...v, message: { state: "sending" }, stopChosen: undefined }));
      try {
        const answer = await client.call("session.send", attachments?.length ? { id, text, attachments } : { id, text });
        const pending = !turnSeen;
        // No turn in the answer: Core has not projected the message, as for one the backend holds behind the running
        // turn (PROTO-CORE-API, Turns and attachments). It waits here by name until a user.message after it shows.
        const turn = (answer as { turn?: unknown } | null | undefined)?.turn;
        const waits = name !== undefined && typeof turn !== "number";
        set((v) => ({
          ...v,
          message: undefined,
          turnPending: pending || undefined,
          ...(waits ? { waitingHere: [...(v.waitingHere ?? []), { name, after }] } : {}),
        }));
        return true;
      } catch (error) {
        // Only Core's cancel is the stop's doing; any other failure, even after a stop, is still shown.
        const stopped = stopping && stoppedBeforeSent(error);
        // A stop that canceled this send stopped no turn: a turn still running can be stopped.
        set((v) => ({ ...v, message: stopped ? undefined : sendFailure(error), stopChosen: stopped ? undefined : v.stopChosen }));
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
      if (view.stopChosen) return;
      if (view.message?.state === "sending") stopping = true;
      set((v) => ({ ...v, stopChosen: true }));
      // Best effort: a turn that ended meanwhile, or a lost connection, leaves nothing to stop. A stop that never
      // reached Core stopped nothing, so a failure of the send after it is still a refusal to show, and stop is
      // offered again.
      await client.call("session.interrupt", { id }).catch(() => {
        stopping = false;
        set((v) => ({ ...v, stopChosen: undefined }));
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

/**
 * What the status line says waits (FLOW-SEND-MESSAGE-R19): how many, from the session's tile, so it survives a reload;
 * and, when one waits and it is the one this window sent, its name. Nothing while nothing waits.
 */
export function waitingOf(count: number | undefined, here: readonly WaitingHere[] | undefined): { count: number; name?: string } | undefined {
  if (count === undefined || count < 1) return undefined;
  const only = count === 1 && here?.length === 1 ? here[0] : undefined;
  return only ? { count, name: only.name } : { count };
}
