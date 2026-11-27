// What the Clean up list decides without drawing anything (FLOW-CLEAN-UP, steps 5 and 6): which items are ticked to
// begin with, what the question counts, and what `cleanup.remove` is sent.

import { describe, expect, it } from "vitest";
import type { CleanupInspectResult } from "../protocol/core-api.gen";
import { type ItemView, changedTo, itemName, losesWork, lostCounts, questionOf, questionTitle, sendsDiscard, tickedByDefault } from "./cleanup";

const base = { project_id: "alpha", token: "t1" };
const workspace = (more: Partial<CleanupInspectResult> = {}): CleanupInspectResult => ({
  ...base,
  id: "ws",
  kind: "workspace",
  workspace_id: "w1",
  path: "D:/wt/alpha/one",
  branch: "ersa/one",
  session_ids: ["s1"],
  uncommitted: 0,
  ignored: 0,
  unshared: 0,
  stranded: 0,
  unrecorded: false,
  holds_repository: false,
  size_bytes: 1024,
  ...more,
});
const leftover = (more: Partial<CleanupInspectResult> = {}) => workspace({ id: "lo", kind: "leftover", session_ids: undefined, ...more });
const branch = (more: Partial<CleanupInspectResult> = {}): CleanupInspectResult => ({ ...base, id: "br", kind: "branch", branch: "ersa/old", unshared: 0, merged: false, ...more });
const folder = (more: Partial<CleanupInspectResult> = {}): CleanupInspectResult => ({ ...base, id: "fo", kind: "folder", path: "D:/wt/alpha/stray", size_bytes: 2048, ...more });
const seen = (inspected: CleanupInspectResult): ItemView => ({ state: "inspected", item: inspected, inspected });

describe("the items ticked to begin with", () => {
  // verifies: FLOW-CLEAN-UP-R16
  it("are exactly those whose removal loses no unsaved work and the merged branches, never a folder", () => {
    expect(tickedByDefault(seen(workspace()))).toBe(true);
    expect(tickedByDefault(seen(leftover()))).toBe(true);
    // Ignored files are no unsaved work, and a branch's own commits stay with it.
    expect(tickedByDefault(seen(workspace({ ignored: 12, unshared: 3 })))).toBe(true);
    expect(tickedByDefault(seen(branch()))).toBe(true);
    expect(tickedByDefault(seen(branch({ unshared: 2, merged: true })))).toBe(true);

    expect(tickedByDefault(seen(workspace({ uncommitted: 1 })))).toBe(false);
    expect(tickedByDefault(seen(workspace({ stranded: 1 })))).toBe(false);
    expect(tickedByDefault(seen(leftover({ unrecorded: true })))).toBe(false);
    expect(tickedByDefault(seen(branch({ unshared: 2 })))).toBe(false);
    expect(tickedByDefault(seen(folder()))).toBe(false);
    expect(tickedByDefault(seen(folder({ size_bytes: undefined })))).toBe(false);
  });

  // verifies: FLOW-CLEAN-UP-R19
  it("leave out an item still being checked and one that couldn't be", () => {
    const item = workspace();
    expect(tickedByDefault({ state: "checking", item })).toBe(false);
    expect(tickedByDefault({ state: "uninspectable", item, message: "timed out" })).toBe(false);
  });

  // verifies: FLOW-CLEAN-UP-R28
  it("leave out a folder that holds a repository of its own, counting it as one that loses unsaved work", () => {
    expect(losesWork(workspace({ holds_repository: true }))).toBe(true);
    expect(tickedByDefault(seen(workspace({ holds_repository: true })))).toBe(false);
    expect(tickedByDefault(seen(leftover({ holds_repository: true })))).toBe(false);
  });
});

describe("what a removal is sent", () => {
  it("is discard exactly where Core would refuse it without, so no branch Core would keep is deleted", () => {
    expect(sendsDiscard(workspace())).toBe(false);
    expect(sendsDiscard(leftover({ unshared: 3, ignored: 4 }))).toBe(false);
    expect(sendsDiscard(workspace({ uncommitted: 1 }))).toBe(true);
    expect(sendsDiscard(workspace({ holds_repository: true }))).toBe(true);
    expect(sendsDiscard(branch())).toBe(false);
    // A squash-merged branch's commits go with it, merged or not.
    expect(sendsDiscard(branch({ unshared: 1, merged: true }))).toBe(true);
    expect(sendsDiscard(folder())).toBe(true);
    expect(sendsDiscard(folder({ size_bytes: undefined }))).toBe(true);
  });
});

describe("the question", () => {
  // verifies: FLOW-CLEAN-UP-R17
  it("names how many folders and branches go, the space freed, and what is lost for good", () => {
    const q = questionOf([workspace({ uncommitted: 3, stranded: 1, size_bytes: 1000 }), leftover({ size_bytes: 24 }), branch({ unshared: 2 }), branch({ unshared: 5, merged: true })]);
    expect(questionTitle(q)).toBe("Clean up 2 folders and 2 branches?");
    expect(q).toMatchObject({ folders: 2, branches: 2, bytes: 1024, files: 3, commits: 3, discards: true, sessions: true });
    expect(questionTitle(questionOf([branch()]))).toBe("Clean up 1 branch?");
    // Nothing lost: the button reads as it always does.
    expect(questionOf([workspace(), branch({ unshared: 5, merged: true })])).toMatchObject({ files: 0, commits: 0, discards: false });
  });

  // verifies: FLOW-CLEAN-UP-R28
  it("counts the folders that go that hold a repository of their own", () => {
    expect(questionOf([workspace({ holds_repository: true }), leftover({ holds_repository: true }), workspace()])).toMatchObject({ repositories: 2, discards: true });
  });

  // verifies: FLOW-CLEAN-UP-R29
  it("counts the folders Ersa couldn't finish looking through, and leaves them out of the space freed", () => {
    const q = questionOf([folder({ size_bytes: undefined }), folder({ size_bytes: 2048 }), workspace({ size_bytes: undefined })]);
    expect(q).toMatchObject({ folders: 3, unfinished: 1, strays: 1, bytes: 2048, discards: true });
  });
});

describe("an item that changed since it was checked", () => {
  it("is read from the refusal's detail, and only from one naming the item", () => {
    const now = workspace({ uncommitted: 2, token: "t2" });
    expect(changedTo(JSON.stringify(now), "ws")).toEqual(now);
    expect(changedTo(JSON.stringify(now), "other")).toBeUndefined();
    expect(changedTo("busy", "ws")).toBeUndefined();
    expect(changedTo(undefined, "ws")).toBeUndefined();
  });
});

describe("what removing an item loses", () => {
  // verifies: FLOW-CLEAN-UP-R17
  it("counts a leftover's branch commits when it goes with discard, since Core then deletes the branch with them", () => {
    expect(lostCounts(leftover({ uncommitted: 2, unshared: 5 }))).toEqual({ files: 2, commits: 5 });
    expect(questionOf([leftover({ uncommitted: 2, unshared: 5, stranded: 1 })])).toMatchObject({ files: 2, commits: 6 });
    // Without discard the branch is kept with its commits; a session's folder is put away, keeping its branch.
    expect(lostCounts(leftover({ unshared: 5 }))).toEqual({ files: 0, commits: 0 });
    expect(lostCounts(workspace({ uncommitted: 2, unshared: 5 }))).toEqual({ files: 2, commits: 0 });
  });
});

describe("an item's name", () => {
  it("is a folder's last part, with either separator", () => {
    expect(itemName(folder({ path: String.raw`C:\Users\you\worktrees\repo\stray-folder` }), [])).toBe("stray-folder");
    expect(itemName(leftover({ path: "D:/wt/alpha/stray/" }), [])).toBe("stray");
    expect(itemName(workspace({ session_ids: ["s1", "s2"] }), [{ id: "s1", title: "One" }])).toBe("One, Untitled session");
  });
});
