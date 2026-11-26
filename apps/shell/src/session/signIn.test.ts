// Which failure a session's sign-in note is for (FLOW-SIGN-IN-R17): the latest error that asks the person to sign in,
// while no message of theirs follows it and nothing since has cleared it.

import { describe, expect, it } from "vitest";
import { sessionNoteFailure } from "./signIn";
import type { Item } from "./transcript";

const loggedOut = (seq: number): Item => ({ kind: "problem", key: `problem:${seq}`, seq, message: "Not logged in · Please run /login", signIn: true });
const otherError = (seq: number): Item => ({ kind: "problem", key: `problem:${seq}`, seq, message: "Overloaded" });
const said = (key: string): Item => ({ kind: "user", key, text: "Fix the build" });
const answered = (key: string): Item => ({ kind: "assistant", key, text: "Done.", final: true });

describe("the failure a session's sign-in note is for", () => {
  // verifies: FLOW-SIGN-IN-R17
  it("is the latest error that asks to sign in, while no message of the person's follows it", () => {
    expect(sessionNoteFailure([said("u1"), loggedOut(3)], -1)).toBe(3);
    // Another kind of error after it leaves it the latest that asks to sign in.
    expect(sessionNoteFailure([said("u1"), loggedOut(3), otherError(5)], -1)).toBe(3);
    // The latest of two.
    expect(sessionNoteFailure([loggedOut(3), said("u2"), loggedOut(8)], -1)).toBe(8);
    // A message of the person's after it: no note, whatever came after that message.
    expect(sessionNoteFailure([loggedOut(3), said("u2")], -1)).toBeUndefined();
    expect(sessionNoteFailure([loggedOut(3), said("u2"), answered("a2")], -1)).toBeUndefined();
    // No such error at all, or only another kind.
    expect(sessionNoteFailure([said("u1"), answered("a1")], -1)).toBeUndefined();
    expect(sessionNoteFailure([otherError(2)], -1)).toBeUndefined();
  });

  // verifies: FLOW-SIGN-IN-R17
  it("is none once the window has cleared it: a send it made, or a sign-in that ended signed in, since it arrived", () => {
    // Cleared through the failure's own event, or past it.
    expect(sessionNoteFailure([loggedOut(3)], 3)).toBeUndefined();
    expect(sessionNoteFailure([loggedOut(3)], 4)).toBeUndefined();
    // Cleared only through an earlier event: the failure came after, and shows.
    expect(sessionNoteFailure([loggedOut(3)], 2)).toBe(3);
  });
});
