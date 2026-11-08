import { describe, expect, it } from "vitest";
import type { Project, Session } from "../protocol/core-api.gen";
import { collapsedOf, treeOf } from "./tree";

const project = (id: string): Project => ({ id, host_id: "local", path: `D:\\${id}`, name: id, vcs: "git", trusted: true });
const session = (id: string, workspace_id: string, created_at: string): Session => ({
  id,
  workspace_id,
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  state: "idle",
  created_at,
  archived: false,
  pinned: false,
  tile: { state: "idle", pending: [] },
});
const ids = (sessions: Session[]) => sessions.map((s) => s.id);

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
  // FLOW-ORGANIZE-SESSIONS-R08 is cited once grouping by none is built and tested too: these cover grouping by project.
  it("puts each session under its own project, across its workspaces, newest first", () => {
    const tree = treeOf(projects, workspaces, sessions);
    expect(tree.projects.map(({ project, sessions }) => [project.id, ids(sessions)])).toEqual([
      ["ersa", ["new", "mid", "old"]],
      ["engine", ["engine"]],
      ["notes", []],
    ]);
    expect(tree.loose).toEqual([]);
  });

  it("keeps the projects in the order Core lists them, whatever their sessions", () => {
    const reversed = [...projects].reverse();
    expect(treeOf(reversed, workspaces, sessions).projects.map(({ project }) => project.id)).toEqual(["notes", "engine", "ersa"]);
  });

  it("keeps a project with no sessions, and every project before the sessions are read", () => {
    expect(treeOf(projects, workspaces, []).projects.map(({ project, sessions }) => [project.id, sessions.length])).toEqual([
      ["ersa", 0],
      ["engine", 0],
      ["notes", 0],
    ]);
  });

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

  // verifies: FLOW-ORGANIZE-SESSIONS-R01
  it("leads with the pinned sessions, newest first, and shows them under neither their project nor the end", () => {
    const pin = (s: Session): Session => ({ ...s, pinned: true });
    const tree = treeOf(projects, workspaces, [
      pin(sessions[0]!),
      sessions[1]!,
      sessions[2]!,
      pin(sessions[3]!),
      pin(session("unknown", "w-gone", "2026-11-01T08:00:00.000Z")),
    ]);
    expect(ids(tree.pinned)).toEqual(["mid", "old", "unknown"]);
    expect(tree.projects.map(({ project, sessions }) => [project.id, ids(sessions)])).toEqual([
      ["ersa", ["new"]],
      ["engine", ["engine"]],
      ["notes", []],
    ]);
    expect(tree.loose).toEqual([]);
    // Pinning changes neither its project nor its section: unpinned, a session is back under its project.
    expect(treeOf(projects, workspaces, sessions).projects[0]?.sessions.map((s) => s.id)).toEqual(["new", "mid", "old"]);
  });

  it("has no pinned sessions while none is pinned", () => {
    expect(treeOf(projects, workspaces, sessions).pinned).toEqual([]);
  });

  it("puts every session at the end when no project could be read", () => {
    const tree = treeOf([], workspaces, sessions);
    expect(tree.projects).toEqual([]);
    expect(ids(tree.loose)).toEqual(["engine", "new", "mid", "old"]);
  });
});

// LAY-SHELL, Collapsing: a layout brief, so no requirement IDs.
describe("what a collapsed heading says", () => {
  const waiting = (id: string, kinds: ("permission" | "question" | "elicitation")[]): Session => ({
    ...session(id, "w", "2026-11-01T09:00:00.000Z"),
    tile: { state: "waiting_user", pending: kinds.map((kind, i) => ({ kind, request_id: `${id}-${i}` }) as Session["tile"]["pending"][number]) },
  });

  it("counts every session it holds, with no mark when none needs the user", () => {
    expect(collapsedOf([session("a", "w", "x"), session("b", "w", "y"), session("c", "w", "z")])).toEqual({ count: 3, tone: undefined });
    expect(collapsedOf([])).toEqual({ count: 0, tone: undefined });
  });

  it("marks the count in the accent when the only wants are questions", () => {
    expect(collapsedOf([session("a", "w", "x"), waiting("q", ["question"]), waiting("q2", ["question", "question"])])).toEqual({ count: 3, tone: "accent" });
  });

  it("marks it caution when any session wants a permission, over a question", () => {
    expect(collapsedOf([waiting("q", ["question"]), waiting("p", ["permission"])])).toEqual({ count: 2, tone: "caution" });
    expect(collapsedOf([waiting("both", ["question", "permission"])])).toEqual({ count: 1, tone: "caution" });
  });

  it("marks a kind of request this window doesn't know as caution", () => {
    expect(collapsedOf([waiting("u", ["elicitation"])])).toEqual({ count: 1, tone: "caution" });
  });
});
