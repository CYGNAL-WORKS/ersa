import { describe, expect, it } from "vitest";
import type { Session } from "../protocol/core-api.gen";
import { forkUnavailable } from "./fork";

const session = (more: Partial<Session> = {}, tile: Partial<Session["tile"]> = {}): Session => ({
  id: "s1",
  workspace_id: "w1",
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  state: "idle",
  created_at: "2026-11-01T09:00:00Z",
  last_activity_at: "2026-11-01T09:05:00Z",
  archived: false,
  pinned: false,
  unseen: false,
  ...more,
  tile: { state: "idle", pending: [], ...tile },
});

describe("when Fork is unavailable, from the session as listed", () => {
  // verifies: FLOW-FORK-SESSION-R03
  it("is available once a reply has finished and nothing runs, idle, ended or archived", () => {
    expect(forkUnavailable(session(), false)).toBeUndefined();
    expect(forkUnavailable(session({ state: "ended" }, { state: "ended" }), false)).toBeUndefined();
    expect(forkUnavailable(session({ archived: true }), false)).toBeUndefined();
  });

  // verifies: FLOW-FORK-SESSION-R03
  it("says it is available once the reply finishes while one runs, waits on the user, or a message is being sent", () => {
    const reason = "Available once the reply finishes";
    expect(forkUnavailable(session({}, { state: "running" }), false)).toBe(reason);
    expect(forkUnavailable(session({}, { state: "starting" }), false)).toBe(reason);
    expect(forkUnavailable(session({}, { state: "waiting_user" }), false)).toBe(reason);
    expect(forkUnavailable(session({}, { pending: [{ kind: "question", request_id: "q1" }] }), false)).toBe(reason);
    expect(forkUnavailable(session({ sending: "copying" }), false)).toBe(reason);
  });

  it("leaves a session with no last activity known to Core, which alone can tell whether a reply has finished", () => {
    expect(forkUnavailable(session({ last_activity_at: undefined }), false)).toBeUndefined();
  });

  it("says why while Ersa can't reach Core, and while a fork of it is on its way", () => {
    expect(forkUnavailable(session(), true)).toBe("Available once Ersa reaches your sessions");
    expect(forkUnavailable(session(), false, { state: "sending" })).toBe("Forking…");
    // A refused fork can be tried again: what refused it may have changed.
    expect(forkUnavailable(session(), false, { state: "failed", message: "no" })).toBeUndefined();
  });
});
