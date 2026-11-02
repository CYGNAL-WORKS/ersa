import { describe, expect, it } from "vitest";
import { type StageAction, type StageState, initialStage, stage } from "./stage";

const run = (actions: StageAction[], from: StageState = initialStage) => actions.reduce(stage, from);

describe("the stage", () => {
  // verifies: FLOW-NEW-SESSION-R01
  it("draws the screen afresh on each New session, so its composer takes focus each time", () => {
    const first = run([{ type: "newSession", projectId: "p1" }]);
    const again = stage(first, { type: "newSession", projectId: "p2" });
    const same = stage(again, { type: "newSession", projectId: "p2" });
    expect(new Set([first.composing?.key, again.composing?.key, same.composing?.key]).size).toBe(3);
    expect(same.composing?.projectId).toBe("p2");
  });

  // verifies: FLOW-NEW-SESSION-R08
  it("keeps the screen as it was when a start fails, with the failure shown", () => {
    const sending = run([{ type: "newSession", projectId: "p1" }, { type: "sending" }]);
    const failed = stage(sending, { type: "failed", message: "the backend could not be started: not logged in" });
    // The same screen, not drawn afresh: its composer, and the message in it, stay.
    expect(failed.composing).toEqual(sending.composing);
    expect(failed.starting).toEqual({ state: "failed", message: "the backend could not be started: not logged in" });
    expect(failed.activeId).toBeUndefined();
  });

  // verifies: FLOW-NEW-SESSION-R09
  it("makes a started session the active one, and shows it once the window follows it", () => {
    const started = run([
      { type: "showNewest", id: "s-old" },
      { type: "newSession", projectId: "p1" },
      { type: "sending" },
      { type: "started", id: "s-new", text: "Fix the build" },
    ]);
    // Still the screen, sending, until the transcript can show: never an empty stage between them.
    expect(started).toMatchObject({ activeId: "s-new", opening: "s-new", starting: { state: "sending" } });
    expect(started.composing).toBeDefined();
    expect(stage(started, { type: "followed", id: "s-old" })).toMatchObject({ opening: "s-new", starting: { state: "sending" } });
    const shown = stage(started, { type: "followed", id: "s-new" });
    expect(shown).toMatchObject({ activeId: "s-new", composing: undefined, starting: undefined, opening: undefined });
  });

  // verifies: FLOW-NEW-SESSION-R09
  it("settles the first session ever started, whose tile arrives before its start does", () => {
    // No session yet: the default screen. While the first message is on its way, Core's tile for the new session
    // makes the window read the list again, which offers the newest session before the start has answered.
    let state = run([{ type: "sending" }, { type: "showNewest", id: "s-new" }]);
    expect(state.activeId).toBeUndefined();
    state = stage(state, { type: "started", id: "s-new", text: "Fix the build" });
    state = stage(state, { type: "followed", id: "s-new" });
    expect(state).toMatchObject({ activeId: "s-new", starting: undefined, opening: undefined, composing: undefined });
    // The stage is free again.
    expect(stage(state, { type: "select", id: "s-other" }).activeId).toBe("s-other");
  });

  it("settles a start at once when the window already follows the session", () => {
    const state = run([{ type: "followed", id: "s1" }, { type: "newSession" }, { type: "sending" }, { type: "started", id: "s1", text: "hi" }]);
    expect(state).toMatchObject({ activeId: "s1", starting: undefined, opening: undefined, composing: undefined });
  });

  // verifies: FLOW-NEW-SESSION-R08
  it("keeps the screen, its failure and its text when the first session ever started fails", () => {
    // The failed start's tile reaches the window before the start answers.
    const state = run([
      { type: "sending" },
      { type: "showNewest", id: "s-doomed" },
      { type: "failed", message: "the backend could not be started: not logged in" },
      { type: "showNewest", id: "s-doomed" },
    ]);
    expect(state.activeId).toBeUndefined();
    expect(state.starting).toEqual({ state: "failed", message: "the backend could not be started: not logged in" });
  });

  it("opens a kept session with why its first message did not go through, and gives the text back when it should", () => {
    const giveBack = run([{ type: "sending" }, { type: "started", id: "s1", text: "Fix it", firstSend: { message: "still starting", giveBack: true } }]);
    expect(giveBack.firstSend).toEqual({ id: "s1", text: "Fix it", message: "still starting" });
    const kept = run([{ type: "sending" }, { type: "started", id: "s1", text: "Fix it", firstSend: { message: "not projected", giveBack: false } }]);
    expect(kept.firstSend).toEqual({ id: "s1", message: "not projected" });
  });

  // verifies: FLOW-NEW-SESSION-R03
  it("leaves the screen for a chosen session, and stays while a start is on its way", () => {
    const composing = run([{ type: "newSession", projectId: "p1" }]);
    expect(stage(composing, { type: "select", id: "s1" })).toMatchObject({ activeId: "s1", composing: undefined });
    const sending = stage(composing, { type: "sending" });
    expect(stage(sending, { type: "select", id: "s1" })).toBe(sending);
    // New session for another project waits too: the start's outcome belongs to the screen it began on.
    expect(stage(sending, { type: "newSession", projectId: "p2" })).toBe(sending);
  });

  it("shows the newest session only when none is chosen", () => {
    expect(run([{ type: "showNewest", id: "s1" }]).activeId).toBe("s1");
    expect(run([{ type: "select", id: "s2" }, { type: "showNewest", id: "s1" }]).activeId).toBe("s2");
  });
});
