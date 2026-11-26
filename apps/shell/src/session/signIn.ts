// Signing in to a backend from inside Ersa (FLOW-SIGN-IN; ADR 0040). The sign-in is the backend's own, on its vendor's
// page in the person's own browser: Ersa asks Core to start or stop it, opens the page's address when it is `https`,
// and follows what the host's listing and Core's `backend.sign_in_ended` say. Nothing a sign-in exchanges passes
// through here but the page's address, which is opened and never written anywhere.

import { useCallback, useRef, useState } from "react";
import { type CoreClient, describeError } from "../core/client";
import type { ShellControl } from "../core/shell";
import type { BackendInfo, BackendSignInEndedNotification, Session } from "../protocol/core-api.gen";
import type { Item } from "./transcript";

/** What a failure adds to the note, kept until the person chooses Sign in or the note goes (FLOW-SIGN-IN-R15). */
export type SignInWords =
  /** Core refused to start it, with Core's reason (R03). */
  | { kind: "refused"; reason: string }
  /** The page's address was not `https`, so Ersa opened nothing (R05). */
  | { kind: "unopened" }
  /** It ended `failed`, with the backend's own reason when Core gave one (R13). */
  | { kind: "failed"; reason?: string }
  /** It ended `timed_out` (R14). */
  | { kind: "timedOut" }
  /** It ended `signed_in`, and the backend still reports that it is signed out (R12). */
  | { kind: "stillSignedOut" };

/**
 * What this window alone knows of a backend's sign-in on a host: that its Sign in is waiting on Core and the reading
 * after (`starting`, R03), that its Cancel is (`canceling`, R07), and what a failure added to the note. Whether a
 * sign-in is under way is the host's listing's to say (R06), never kept here.
 */
export interface SignInLocal {
  phase?: "starting" | "canceling";
  said?: SignInWords;
}

/** What the sign-in note shows. */
export type NoteState = { kind: "signedOut"; said?: SignInWords } | { kind: "starting" } | { kind: "waiting" } | { kind: "canceling" };

/** Each backend's local sign-in state, by `signInKey`. */
export type SignIns = Readonly<Record<string, SignInLocal>>;

/** The key a backend on a host is held under. A backend's name holds no line break. */
export const signInKey = (hostId: string, name: string) => `${hostId}\n${name}`;

/**
 * The backend the new-session screen's sign-in note is for: while no backend on the host can run, the first the
 * listing reports `signed_out` (FLOW-SIGN-IN-R01); none otherwise.
 */
export function noteBackend(listing: readonly BackendInfo[] | undefined): BackendInfo | undefined {
  if (!listing || listing.some((b) => b.status === "available")) return undefined;
  return listing.find((b) => b.status === "signed_out");
}

/**
 * What the note shows for `backend`: Starting… while this window's Sign in waits (R03); signed out again at once
 * after Cancel (R07), with Sign in held until Core has answered the Cancel and the backends are read again, since a
 * Sign in that reached Core first would join the sign-in on its way out (PROTO-CORE-API-R156); waiting while the
 * listing reports a sign-in under way (R06); else signed out, with what a failure added.
 */
export function noteState(backend: Pick<BackendInfo, "signing_in">, local: SignInLocal | undefined): NoteState {
  if (local?.phase === "starting") return { kind: "starting" };
  if (local?.phase === "canceling") return { kind: "canceling" };
  if (backend.signing_in) return { kind: "waiting" };
  return { kind: "signedOut", said: local?.said };
}

/**
 * The failure a session's sign-in note is for (FLOW-SIGN-IN-R17), by its event's `seq`: the latest error in the
 * session's transcript that asks the person to sign in (`action: log_in`), while no message of theirs follows it and
 * it came after `cleared`, the latest event this window knew of when it last cleared the note (a send Core accepted,
 * or a sign-in for the session's backend that ended `signed_in`). None otherwise.
 */
export function sessionNoteFailure(items: readonly Item[], cleared: number): number | undefined {
  for (let i = items.length - 1; i >= 0; i -= 1) {
    const item = items[i]!;
    // A message of the person's after any such failure: the note has gone with it. One the backend or another
    // session wrote is not theirs.
    if (item.kind === "user" && item.origin === undefined) return undefined;
    if (item.kind === "problem" && item.signIn) return item.seq > cleared ? item.seq : undefined;
  }
  return undefined;
}

/**
 * Where this window has cleared each session's sign-in note (FLOW-SIGN-IN-R17), by session id: the latest event of the
 * session it knew of then (`sessionNoteFailure`'s `cleared`). A session's event numbers count within one view of it
 * (PROTO-AGENT-EVENTS, Opening a session), and a live session keeps its view while Core runs it, so a mark holds only
 * while the session stays live on this connection: `keep` drops the rest, and a lost connection drops them all, so a
 * window that reconnects shows a session's note again (FLOW-SIGN-IN, Open questions).
 */
export function useNoteClears() {
  const [clears, setClears] = useState<Readonly<Record<string, number>>>({});
  /** Clears a session's note through `seq`, never back. */
  const clear = useCallback((marks: Readonly<Record<string, number>>) => {
    setClears((all) => {
      let next: Record<string, number> | undefined;
      for (const [id, seq] of Object.entries(marks)) {
        if ((all[id] ?? -1) >= seq) continue;
        next ??= { ...all };
        next[id] = seq;
      }
      return next ?? all;
    });
  }, []);
  /** Keeps only the marks of the sessions `live` names; none when the connection is lost. */
  const keep = useCallback((live: ReadonlySet<string>) => {
    setClears((all) => (Object.keys(all).every((id) => live.has(id)) ? all : Object.fromEntries(Object.entries(all).filter(([id]) => live.has(id)))));
  }, []);
  return { clears, clear, keep };
}

/**
 * The marks a sign-in that ended `signed_in` for `name` on `hostId` sets (R17): every session of that backend on that
 * host is cleared through the latest event the window knows of it, its listed `last_event_seq` or, for the session
 * shown, its transcript's, whichever is later, so only a failure that arrives after the sign-in shows the note again.
 */
export function signedInClears(
  hostId: string,
  name: string,
  sessions: readonly Pick<Session, "id" | "host_id" | "backend" | "last_event_seq">[],
  shown?: { id: string; lastSeq: number },
): Record<string, number> {
  const marks: Record<string, number> = {};
  for (const s of sessions) {
    if (s.host_id !== hostId || s.backend !== name) continue;
    marks[s.id] = Math.max(s.last_event_seq ?? -1, shown?.id === s.id ? shown.lastSeq : -1);
  }
  return marks;
}

/** Whether Ersa may open `url` as a sign-in page: only over `https` (FLOW-SIGN-IN-R04, R05). */
export function openableSignInPage(url: string): boolean {
  try {
    return new URL(url).protocol === "https:";
  } catch {
    return false;
  }
}

/** Reads a host's backends anew, past any reading already out; resolves with the listing, or none when it failed. */
export type ReadFresh = (hostId: string) => Promise<readonly BackendInfo[] | undefined>;

/**
 * The window's sign-ins: Sign in and Cancel for a backend on a host, and Core's word that one ended. Every step that
 * changes what the host has under way reads its backends again, so the note follows the listing in every window.
 */
export function useSignIns(client: Pick<CoreClient, "call">, openLink: ShellControl["openLink"] | undefined, readFresh: ReadFresh) {
  const [signIns, setSignIns] = useState<SignIns>({});
  // Each Sign in and Cancel takes a turn, so a step that ends after a newer one leaves the newer one's phase alone.
  const turns = useRef(new Map<string, number>());
  const take = (key: string) => {
    const turn = (turns.current.get(key) ?? 0) + 1;
    turns.current.set(key, turn);
    return turn;
  };
  const mine = (key: string, turn: number) => turns.current.get(key) === turn;
  const update = useCallback((key: string, change: (was: SignInLocal) => SignInLocal) => {
    setSignIns((all) => ({ ...all, [key]: change(all[key] ?? {}) }));
  }, []);
  // A step's end clears its phase and adds what it says; a newer Sign in or Cancel since has its own to say.
  const settle = (key: string, turn: number, said?: SignInWords) =>
    update(key, (was) => (mine(key, turn) ? { said: said ?? was.said } : was));

  const signIn = async (hostId: string, name: string) => {
    const key = signInKey(hostId, name);
    const turn = take(key);
    // Choosing Sign in clears what a failure said (R15), and Sign in waits on Core (R03).
    update(key, () => ({ phase: "starting" }));
    let url: string | undefined;
    try {
      ({ url } = await client.call("backend.sign_in", { host_id: hostId, name }));
    } catch (error) {
      settle(key, turn, { kind: "refused", reason: describeError(error) });
      return;
    }
    let said: SignInWords | undefined;
    if (url !== undefined) {
      if (openLink && openableSignInPage(url)) {
        // In the person's own browser, the system's default (R04).
        openLink(url);
      } else {
        // Any other scheme opens nothing, and the sign-in it started is stopped (R05). Waited on, so the reading
        // below no longer reports it under way.
        said = { kind: "unopened" };
        await client.call("backend.cancel_sign_in", { host_id: hostId, name }).catch(() => {});
      }
    }
    // Core has started it: the host's backends now report it under way (R09). Sign in stays Starting… until they
    // have been read (R03), so the note never shows Sign in again with the page open.
    await readFresh(hostId);
    settle(key, turn, said);
  };

  const cancel = async (hostId: string, name: string) => {
    const key = signInKey(hostId, name);
    const turn = take(key);
    // The note says the backend isn't signed in again at once, and offers Sign in (R07).
    update(key, (was) => ({ ...was, phase: "canceling" }));
    await client.call("backend.cancel_sign_in", { host_id: hostId, name }).catch(() => {});
    await readFresh(hostId);
    settle(key, turn);
  };

  // A sign-in under way ended, in this window's or another's (PROTO-CORE-API-R159): the backends are read again (R09),
  // and the note says how it ended, except for a cancel, which adds nothing (R14).
  const ended = async (ending: BackendSignInEndedNotification) => {
    const key = signInKey(ending.host_id, ending.name);
    if (ending.outcome === "failed") update(key, (was) => ({ ...was, said: { kind: "failed", reason: ending.message } }));
    if (ending.outcome === "timed_out") update(key, (was) => ({ ...was, said: { kind: "timedOut" } }));
    const listing = await readFresh(ending.host_id);
    // Signed in, yet the backend still says it is signed out: the note says so, never as if nothing happened (R12).
    if (ending.outcome === "signed_in" && listing?.find((b) => b.name === ending.name)?.status === "signed_out") {
      update(key, (was) => ({ ...was, said: { kind: "stillSignedOut" } }));
    }
  };

  // The new-session screen's note went for this host's listing: what a failure said about any backend no note is for
  // goes with it (R15). `kept`, by `signInKey`, is the backend the shown session's note is for (R17), whose words stay
  // while that note does, whatever the listing says.
  const listed = useCallback((hostId: string, listing: readonly BackendInfo[], kept?: string) => {
    const shown = noteBackend(listing)?.name;
    setSignIns((all) => {
      let next: Record<string, SignInLocal> | undefined;
      for (const [key, local] of Object.entries(all)) {
        if (!key.startsWith(`${hostId}\n`) || local.said === undefined || key === signInKey(hostId, shown ?? "") || key === kept) continue;
        next ??= { ...all };
        next[key] = { ...local, said: undefined };
      }
      return next ?? all;
    });
  }, []);

  // A session's note shows for a failure it has not shown for before: what an earlier sign-in's failure said is not
  // about this one, and goes (R15), so the note says only that the backend isn't signed in.
  const forget = useCallback((key: string) => {
    setSignIns((all) => (all[key]?.said === undefined ? all : { ...all, [key]: { ...all[key], said: undefined } }));
  }, []);

  return { signIns, signIn, cancel, ended, listed, forget };
}
