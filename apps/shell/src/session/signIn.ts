// Signing in to a backend from inside Ersa (FLOW-SIGN-IN; ADR 0040). The sign-in is the backend's own, on its vendor's
// page in the person's own browser: Ersa asks Core to start or stop it, opens the page's address when it is `https`,
// and follows what the host's listing and Core's `backend.sign_in_ended` say. Nothing a sign-in exchanges passes
// through here but the page's address, which is opened and never written anywhere.

import { useCallback, useRef, useState } from "react";
import { type CoreClient, describeError } from "../core/client";
import type { ShellControl } from "../core/shell";
import type { BackendInfo, BackendSignInEndedNotification } from "../protocol/core-api.gen";

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
export function noteState(backend: BackendInfo, local: SignInLocal | undefined): NoteState {
  if (local?.phase === "starting") return { kind: "starting" };
  if (local?.phase === "canceling") return { kind: "canceling" };
  if (backend.signing_in) return { kind: "waiting" };
  return { kind: "signedOut", said: local?.said };
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

  // The note went for this host's listing: what a failure said about any backend the note is no longer for goes with
  // it (R15).
  const listed = useCallback((hostId: string, listing: readonly BackendInfo[]) => {
    const shown = noteBackend(listing)?.name;
    setSignIns((all) => {
      let next: Record<string, SignInLocal> | undefined;
      for (const [key, local] of Object.entries(all)) {
        if (!key.startsWith(`${hostId}\n`) || local.said === undefined || key === signInKey(hostId, shown ?? "")) continue;
        next ??= { ...all };
        next[key] = { ...local, said: undefined };
      }
      return next ?? all;
    });
  }, []);

  return { signIns, signIn, cancel, ended, listed };
}
