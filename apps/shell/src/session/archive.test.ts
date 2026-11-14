import { describe, expect, it } from "vitest";
import { tileActions } from "../components/TileMenu";
import type { Session } from "../protocol/core-api.gen";
import { archiveUnavailable } from "./archive";

const session = (more: Partial<Session> = {}, tile: Partial<Session["tile"]> = {}): Session => ({
  id: "s1",
  workspace_id: "w1",
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  state: "idle",
  created_at: "2026-11-01T09:00:00Z",
  archived: false,
  pinned: false,
  unseen: false,
  ...more,
  tile: { state: "idle", pending: [], ...tile },
});

const busyReason = "Stop the session to archive it";

describe("when Archive is unavailable, from the session as listed", () => {
  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R11
  it("says to stop the session while it is starting, working, waiting on the user, or being sent a message", () => {
    expect(archiveUnavailable(session({}, { state: "starting" }), false)).toBe(busyReason);
    expect(archiveUnavailable(session({}, { state: "running" }), false)).toBe(busyReason);
    expect(archiveUnavailable(session({}, { state: "waiting_user" }), false)).toBe(busyReason);
    expect(archiveUnavailable(session({}, { pending: [{ kind: "question", request_id: "q1" }] }), false)).toBe(
      busyReason,
    );
    expect(archiveUnavailable(session({ sending: "copying" }), false)).toBe(busyReason);
  });

  it("is available while idle or ended, and Unarchive always is", () => {
    expect(archiveUnavailable(session(), false)).toBeUndefined();
    expect(archiveUnavailable(session({ state: "ended" }, { state: "ended" }), false)).toBeUndefined();
    expect(archiveUnavailable(session({ archived: true }, { state: "running" }), false)).toBeUndefined();
  });

  it("is unavailable while Ersa can't reach Core, or while the last choice is on its way", () => {
    expect(archiveUnavailable(session(), true)).toBe("Available once Ersa reaches your sessions");
    expect(archiveUnavailable(session(), false, { state: "sending" })).toBe("Saving…");
    expect(archiveUnavailable(session(), false, { state: "failed", message: "no" })).toBeUndefined();
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R11
  it("is what the tile's menu offers", () => {
    const actions = tileActions(session({}, { state: "running" }), { onArchive: () => {} });
    expect(actions.archive?.unavailable).toBe(busyReason);
    const archiving = tileActions(session(), { onArchive: () => {}, archiving: { s1: { state: "sending" } } });
    expect(archiving.archive?.unavailable).toBe("Saving…");
  });
});
