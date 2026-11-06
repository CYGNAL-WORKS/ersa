import { describe, expect, it } from "vitest";
import type { Project, Session } from "../protocol/core-api.gen";
import { newestFirst, treeOf } from "./tree";

const project = (id: string): Project => ({ id, host_id: "local", path: `D:\\${id}`, name: id, vcs: "git", trusted: true });
const session = (id: string, workspace_id: string, created_at: string): Session => ({
  id,
  workspace_id,
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  state: "idle",
  created_at,
  tile: { state: "idle", pending: [] },
});
const ids = (sessions: Session[]) => sessions.map((s) => s.id);

describe("newestFirst", () => {
  it("orders by creation time, newest first, without changing its input", () => {
    const sessions = [session("a", "w", "2026-11-01T09:00:00.000Z"), session("b", "w", "2026-11-01T10:00:00.000Z"), session("c", "w", "2026-11-01T09:30:00.000Z")];
    expect(ids(newestFirst(sessions))).toEqual(["b", "c", "a"]);
    expect(ids(sessions)).toEqual(["a", "b", "c"]);
  });
});

describe("the rail's tree", () => {
  const projects = [project("ersa"), project("engine"), project("notes")];
  // Two workspaces of ersa and one of engine; notes has none with a session.
  const workspaces = { "w-root": { project_id: "ersa" }, "w-fix": { project_id: "ersa" }, "w-engine": { project_id: "engine" } };
  const sessions = [
    session("old", "w-root", "2026-11-01T09:00:00.000Z"),
    session("engine", "w-engine", "2026-11-01T12:00:00.000Z"),
    session("new", "w-fix", "2026-11-01T11:00:00.000Z"),
    session("mid", "w-root", "2026-11-01T10:00:00.000Z"),
  ];

  // verifies: FLOW-NEW-SESSION-R13
  // verifies: FLOW-ORGANIZE-SESSIONS-R08 (grouped by project; grouping by none arrives with the arrange menu)
  it("puts each session under its own project, across its workspaces, newest first", () => {
    const tree = treeOf(projects, workspaces, sessions);
    expect(tree.projects.map(({ project, sessions }) => [project.id, ids(sessions)])).toEqual([
      ["ersa", ["new", "mid", "old"]],
      ["engine", ["engine"]],
      ["notes", []],
    ]);
    expect(tree.loose).toEqual([]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R08
  it("keeps the projects in the order Core lists them, whatever their sessions", () => {
    const reversed = [...projects].reverse();
    expect(treeOf(reversed, workspaces, sessions).projects.map(({ project }) => project.id)).toEqual(["notes", "engine", "ersa"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R08
  it("keeps a project with no sessions, and every project before the sessions are read", () => {
    expect(treeOf(projects, workspaces, []).projects.map(({ project, sessions }) => [project.id, sessions.length])).toEqual([
      ["ersa", 0],
      ["engine", 0],
      ["notes", 0],
    ]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R08
  it("puts a session whose project it can't tell at the end, newest first, rather than drop it", () => {
    // w-gone is not known (its project's workspaces were not read); w-other names a project Core does not list.
    const tree = treeOf(projects, { ...workspaces, "w-other": { project_id: "removed" } }, [
      ...sessions,
      session("unknown", "w-gone", "2026-11-01T08:00:00.000Z"),
      session("orphan", "w-other", "2026-11-01T13:00:00.000Z"),
    ]);
    expect(ids(tree.loose)).toEqual(["orphan", "unknown"]);
    expect(tree.projects.flatMap(({ sessions }) => ids(sessions))).not.toContain("unknown");
  });

  it("puts every session at the end when no project could be read", () => {
    const tree = treeOf([], workspaces, sessions);
    expect(tree.projects).toEqual([]);
    expect(ids(tree.loose)).toEqual(["engine", "new", "mid", "old"]);
  });
});
