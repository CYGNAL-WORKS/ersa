// Which sessions are unseen (LAY-SHELL, The rail: Unseen). LAY-SHELL is a layout brief and carries no requirement IDs.

import { describe, expect, it } from "vitest";
import type { Session } from "../protocol/core-api.gen";
import { noneUnseen, unseenAfter } from "./unseen";

type Tile = Session["tile"];

const s = (id: string, state: string, pending: Tile["pending"] = []): Session => ({
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
  unseen: false,
  tile: { state: state as Tile["state"], pending },
});

// Reports each list in turn, from nothing unseen, with `activeId` active throughout unless a step names its own.
const run = (steps: [Session[], string?][], activeId?: string) =>
  steps.reduce((u, [sessions, active]) => unseenAfter(u, sessions, active ?? activeId), noneUnseen);

describe("unseen sessions", () => {
  it.each(["idle", "waiting_user"])("marks a session that is not active and goes from working to %s", (state) => {
    expect([...run([[[s("a", "running")]], [[s("a", state)]]], "b").ids]).toEqual(["a"]);
  });

  it("keeps the mark while the session stays idle, and through later reports", () => {
    const u = run([[[s("a", "running")]], [[s("a", "idle")]], [[s("a", "idle")]], [[s("a", "running")]]], "b");
    expect([...u.ids]).toEqual(["a"]);
  });

  it("never marks the active session", () => {
    expect(run([[[s("a", "running")]], [[s("a", "idle")]]], "a").ids.size).toBe(0);
  });

  it("clears the mark when the session is made active, and it stays clear after", () => {
    const u = run([[[s("a", "running")]], [[s("a", "idle")]], [[s("a", "idle")], "a"], [[s("a", "idle")], "b"]], "b");
    expect(u.ids.size).toBe(0);
  });

  it("does not mark a session that was idle all along, or that starts and goes idle without working", () => {
    expect(run([[[s("a", "idle")]], [[s("a", "idle")]]], "b").ids.size).toBe(0);
    expect(run([[[s("a", "starting")]], [[s("a", "idle")]]], "b").ids.size).toBe(0);
  });

  it.each(["disconnected", "error", "ended", "paused"])("does not mark a session that goes from working to %s", (state) => {
    expect(run([[[s("a", "running")]], [[s("a", state)]]], "b").ids.size).toBe(0);
  });

  it("does not mark a session that stops working because it needs the user", () => {
    const needs = s("a", "waiting_user", [{ kind: "permission", request_id: "p" }]);
    expect(run([[[s("a", "running")]], [[needs]]], "b").ids.size).toBe(0);
  });

  it("forgets a session that leaves the list", () => {
    expect(run([[[s("a", "running")]], [[s("a", "idle")]], [[]]], "b").ids.size).toBe(0);
  });
});
