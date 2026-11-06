import { describe, expect, it } from "vitest";
import type { Workspace } from "../protocol/core-api.gen";
import { mergeWorkspaces } from "./workspaces";

const ws = (id: string, project_id: string): Workspace => ({ id, project_id, kind: "isolated", path: `D:\ws\${id}`, vcs_ref: id, state: "ready" });

describe("the workspaces the window knows", () => {
  it("keep one a start added when an older read answers without it", () => {
    const known = { "w-root": ws("w-root", "p1"), "w-new": ws("w-new", "p1") };
    const merged = mergeWorkspaces(known, [{ id: "p1" }], [ws("w-root", "p1")]);
    expect(Object.keys(merged).sort()).toEqual(["w-new", "w-root"]);
  });

  it("take what each read lists, keep what a failed read cannot say, and let go of projects no longer listed", () => {
    const known = { a: ws("a", "p1"), b: ws("b", "p2"), gone: ws("gone", "p3") };
    const projects = [{ id: "p1" }, { id: "p2" }];
    const read = mergeWorkspaces(known, projects, [{ ...ws("a", "p1"), vcs_ref: "renamed" }]);
    expect(read.a?.vcs_ref).toBe("renamed");
    const merged = mergeWorkspaces(read, projects, undefined);
    expect(merged.b).toBeDefined();
    expect(merged.gone).toBeUndefined();
  });
});
