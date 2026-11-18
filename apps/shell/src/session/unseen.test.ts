// Ersa's own unseen mark, as what it tells Core (FLOW-ORGANIZE-SESSIONS, Unseen; LAY-SHELL, Unseen). The App's test
// against a fake Core covers the mark surviving a restart (FLOW-ORGANIZE-SESSIONS-R25).

import { describe, expect, it } from "vitest";
import type { Session } from "../protocol/core-api.gen";
import { type UnseenWrite, unseenWrites } from "./unseen";

type Tile = Session["tile"];

const s = (id: string, state: string, more: { pending?: Tile["pending"]; unseen?: boolean } = {}): Session => ({
  id,
  workspace_id: "w",
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  title: id,
  state: state as Tile["state"],
  created_at: "2026-11-01T09:00:00Z",
  archived: false,
  pinned: false,
  unseen: more.unseen ?? false,
  tile: { state: state as Tile["state"], pending: more.pending ?? [] },
});

// Reports each list in turn, with `activeId` active throughout unless a step names its own, and gathers every write.
const run = (steps: [Session[], string?][], activeId?: string, sending: ReadonlySet<string> = new Set()) => {
  let working: ReadonlySet<string> = new Set();
  const all: UnseenWrite[] = [];
  for (const [sessions, active] of steps) {
    const next = unseenWrites(working, sessions, active ?? activeId, sending);
    working = next.working;
    all.push(...next.writes);
  }
  return all;
};

describe("Ersa's unseen mark", () => {
  it.each(["idle", "waiting_user"])("marks a session that is not active and goes from working to %s", (state) => {
    expect(run([[[s("a", "running")]], [[s("a", state)]]], "b")).toEqual([{ id: "a", unseen: true }]);
  });

  it("marks it once, though Core keeps reporting it idle", () => {
    expect(run([[[s("a", "running")]], [[s("a", "idle")]], [[s("a", "idle", { unseen: true })]], [[s("a", "idle", { unseen: true })]]], "b")).toEqual([
      { id: "a", unseen: true },
    ]);
  });

  it("sends nothing for a session already unseen, as the user may have marked it while it worked", () => {
    expect(run([[[s("a", "running", { unseen: true })]], [[s("a", "idle", { unseen: true })]]], "b")).toEqual([]);
  });

  it("never marks the active session", () => {
    expect(run([[[s("a", "running")]], [[s("a", "idle")]]], "a")).toEqual([]);
  });

  it("marks the active session seen when Core reports it unseen, as when an unseen session is made active", () => {
    expect(run([[[s("a", "idle", { unseen: true })], "b"], [[s("a", "idle", { unseen: true })], "a"]])).toEqual([{ id: "a", unseen: false }]);
  });

  it("sends nothing for a session with a change on its way", () => {
    const sending = new Set(["a"]);
    expect(run([[[s("a", "running")]], [[s("a", "idle")]]], "b", sending)).toEqual([]);
    expect(run([[[s("a", "idle", { unseen: true })]]], "a", sending)).toEqual([]);
  });

  it("marks a session whose turn ended while a change to it was on its way, from the next list", () => {
    let working: ReadonlySet<string> = unseenWrites(new Set(), [s("a", "running")], "b", new Set()).working;
    working = unseenWrites(working, [s("a", "idle")], "b", new Set(["a"])).working;
    expect(unseenWrites(working, [s("a", "idle")], "b", new Set()).writes).toEqual([{ id: "a", unseen: true }]);
  });

  it("does not ask again for a session whose last change Core refused", () => {
    const refused = new Set(["a"]);
    expect(unseenWrites(new Set(["a"]), [s("a", "idle")], "b", new Set(), refused).writes).toEqual([]);
    expect(unseenWrites(new Set(), [s("a", "idle", { unseen: true })], "a", new Set(), refused).writes).toEqual([]);
  });

  it("does not mark a session that was idle all along, or that starts and goes idle without working", () => {
    expect(run([[[s("a", "idle")]], [[s("a", "idle")]]], "b")).toEqual([]);
    expect(run([[[s("a", "starting")]], [[s("a", "idle")]]], "b")).toEqual([]);
  });

  it.each(["disconnected", "error", "ended", "paused"])("does not mark a session that goes from working to %s", (state) => {
    expect(run([[[s("a", "running")]], [[s("a", state)]]], "b")).toEqual([]);
  });

  it("does not mark a session that stops working because it needs the user", () => {
    const needs = s("a", "waiting_user", { pending: [{ kind: "permission", request_id: "p" }] });
    expect(run([[[s("a", "running")]], [[needs]]], "b")).toEqual([]);
  });

  it("does not count a running session with a request pending as working, so answering it and going idle marks nothing", () => {
    const pending: Tile["pending"] = [{ kind: "question", request_id: "q" }];
    expect(run([[[s("a", "running", { pending })]], [[s("a", "idle")]]], "b")).toEqual([]);
  });

  it("forgets a session that leaves the list", () => {
    expect(run([[[s("a", "running")]], [[]], [[s("a", "idle")]]], "b")).toEqual([]);
  });
});
