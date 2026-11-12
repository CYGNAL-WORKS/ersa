import { describe, expect, it } from "vitest";
import type { Project, Session } from "../protocol/core-api.gen";
import { type Tree, collapsedOf, dateGroupOf, treeOf } from "./tree";

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
const byGroup = (tree: Tree) => tree.groups.map((g) => [g.key, ids(g.sessions)]);

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
  // verifies: FLOW-ORGANIZE-SESSIONS-R08
  it("puts each session under its own project, across its workspaces, newest first", () => {
    const tree = treeOf(projects, workspaces, sessions);
    expect(byGroup(tree)).toEqual([
      ["ersa", ["new", "mid", "old"]],
      ["engine", ["engine"]],
      ["notes", []],
    ]);
    expect(tree.loose).toEqual([]);
  });

  it("leaves archived sessions out while showing active sessions, its default", () => {
    const archived = sessions.map((s) => (s.id === "mid" || s.id === "engine" ? { ...s, archived: true } : s));
    const tree = treeOf(projects, workspaces, [...archived, { ...session("lost", "w-gone", "2026-11-01T13:00:00.000Z"), archived: true }]);
    expect(byGroup(tree)).toEqual([
      ["ersa", ["new", "old"]],
      ["engine", []],
      ["notes", []],
    ]);
    expect(tree.loose).toEqual([]);
  });

  it("keeps the projects in the order Core lists them, whatever their sessions", () => {
    const reversed = [...projects].reverse();
    expect(treeOf(reversed, workspaces, sessions).groups.map((g) => g.key)).toEqual(["notes", "engine", "ersa"]);
  });

  it("keeps a project with no sessions, and every project before the sessions are read", () => {
    expect(treeOf(projects, workspaces, []).groups.map((g) => [g.key, g.sessions.length])).toEqual([
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
    expect(tree.groups.flatMap((g) => ids(g.sessions))).not.toContain("unknown");
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
    expect(byGroup(tree)).toEqual([
      ["ersa", ["new"]],
      ["engine", ["engine"]],
      ["notes", []],
    ]);
    expect(tree.loose).toEqual([]);
    // Pinning changes neither its project nor its section: unpinned, a session is back under its project.
    expect(treeOf(projects, workspaces, sessions).groups[0]?.sessions.map((s) => s.id)).toEqual(["new", "mid", "old"]);
  });

  it("has no pinned sessions while none is pinned", () => {
    expect(treeOf(projects, workspaces, sessions).pinned).toEqual([]);
  });

  it("puts every session at the end when no project could be read", () => {
    const tree = treeOf([], workspaces, sessions);
    expect(tree.groups).toEqual([]);
    expect(ids(tree.loose)).toEqual(["engine", "new", "mid", "old"]);
  });
});

describe("the rail's tree as the user arranges it", () => {
  const projects = [project("ersa"), project("engine")];
  const workspaces = { "w-ersa": { project_id: "ersa" }, "w-engine": { project_id: "engine" } };
  // Local times, so the tests hold in any time zone: today is 7 November 2026, a little after midnight.
  const now = new Date(2026, 10, 7, 0, 30);
  const at = (day: number, hour = 12) => new Date(2026, 10, day, hour).toISOString();
  const archived = (s: Session): Session => ({ ...s, archived: true });
  const pinned = (s: Session): Session => ({ ...s, pinned: true });

  // verifies: FLOW-ORGANIZE-SESSIONS-R07
  it("groups by the local date each session was created: Today, Yesterday, Previous 7 days and Older, only those with sessions", () => {
    const sessions = [
      session("just-now", "w-ersa", new Date(2026, 10, 7, 0, 5).toISOString()),
      session("late-last-night", "w-engine", new Date(2026, 10, 6, 23, 55).toISOString()),
      // Work moves no session: only the day it was created counts.
      { ...session("two-days", "w-ersa", at(5)), last_activity_at: now.toISOString() },
      session("seven-days", "w-gone", at(0, 1)),
      { ...session("eight-days", "w-ersa", new Date(2026, 9, 30, 23, 0).toISOString()), last_activity_at: now.toISOString() },
    ];
    const tree = treeOf(projects, workspaces, sessions, { groupBy: "date", sortBy: "created", show: "active", now });
    expect(byGroup(tree)).toEqual([
      ["today", ["just-now"]],
      ["yesterday", ["late-last-night"]],
      ["previous_7_days", ["two-days", "seven-days"]],
      ["older", ["eight-days"]],
    ]);
    expect(tree.groups.every((g) => g.kind === "date")).toBe(true);
    expect(tree.loose).toEqual([]);
    // Only the groups with sessions are there.
    expect(byGroup(treeOf(projects, workspaces, [sessions[0]!, sessions[4]!], { groupBy: "date", sortBy: "created", show: "active", now }))).toEqual([
      ["today", ["just-now"]],
      ["older", ["eight-days"]],
    ]);
  });

  it("puts a session created later than now in Today, and one it can't date in Older", () => {
    expect(dateGroupOf(new Date(2026, 10, 9).toISOString(), now)).toBe("today");
    expect(dateGroupOf("not a date", now)).toBe("older");
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R08
  it("grouped by none, shows one list with no headings after the Pinned group", () => {
    const sessions = [session("a", "w-ersa", at(1)), pinned(session("p", "w-engine", at(2))), session("b", "w-gone", at(3)), session("c", "w-engine", at(4))];
    const tree = treeOf(projects, workspaces, sessions, { groupBy: "none", sortBy: "created", show: "active", now });
    expect(ids(tree.pinned)).toEqual(["p"]);
    expect(tree.groups).toEqual([]);
    expect(ids(tree.loose)).toEqual(["c", "b", "a"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R11
  it("shows active sessions only, archived sessions only, or both, the Pinned group included", () => {
    const sessions = [
      session("live", "w-ersa", at(1)),
      archived(session("gone", "w-ersa", at(2))),
      pinned(archived(session("pinned-gone", "w-engine", at(3)))),
      pinned(session("pinned-live", "w-engine", at(4))),
    ];
    const arranged = (show: "active" | "archived" | "all") => treeOf(projects, workspaces, sessions, { groupBy: "project", sortBy: "created", show, now });
    expect([ids(arranged("active").pinned), byGroup(arranged("active"))]).toEqual([["pinned-live"], [["ersa", ["live"]], ["engine", []]]]);
    // Showing archived or all, only projects with sessions shown have a heading (LAY-SHELL, Projects).
    expect([ids(arranged("archived").pinned), byGroup(arranged("archived"))]).toEqual([["pinned-gone"], [["ersa", ["gone"]]]]);
    expect([ids(arranged("all").pinned), byGroup(arranged("all"))]).toEqual([["pinned-live", "pinned-gone"], [["ersa", ["gone", "live"]]]]);
  });
});

describe("the tiles' order within a group", () => {
  const projects = [project("ersa"), project("engine")];
  const workspaces = { w1: { project_id: "ersa" }, w2: { project_id: "engine" } };
  const made = (id: string, hour: number, more: Partial<Session> = {}, workspace = "w1"): Session => ({
    ...session(id, workspace, `2026-11-01T${String(hour).padStart(2, "0")}:00:00.000Z`),
    ...more,
  });
  const arranged = (sessions: Session[], sortBy: "created" | "last_activity" | "name", groupBy: "project" | "date" | "none" = "none") =>
    treeOf(projects, workspaces, sessions, { groupBy, sortBy, show: "active", now: new Date("2026-11-01T15:00:00.000Z") });

  // verifies: FLOW-ORGANIZE-SESSIONS-R09
  it("sorts by date created, newest first", () => {
    expect(ids(arranged([made("a", 9), made("c", 11), made("b", 10)], "created").loose)).toEqual(["c", "b", "a"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R09
  it("sorts by last activity, newest first, taking a session's creation until it has one", () => {
    const sessions = [
      made("worked", 8, { last_activity_at: "2026-11-01T14:00:00.000Z" }),
      made("fresh", 13, { created_at: "2026-11-01T13:45:00.000Z" }),
      made("stale", 13, { last_activity_at: "2026-11-01T13:30:00.000Z" }),
    ];
    expect(ids(arranged(sessions, "last_activity").loose)).toEqual(["worked", "fresh", "stale"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R09
  it("orders times, not their text, when a backend's time has no fraction", () => {
    // As text "12:00:00Z" sorts after "12:00:00.500Z", though it is earlier.
    const plain = { ...made("plain", 12), created_at: "2026-11-01T12:00:00Z" };
    const fraction = { ...made("fraction", 12), created_at: "2026-11-01T12:00:00.500Z" };
    expect(ids(arranged([plain, fraction], "created").loose)).toEqual(["fraction", "plain"]);
    expect(ids(arranged([plain, fraction], "last_activity").loose)).toEqual(["fraction", "plain"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R09
  it("sorts by name in locale order, with untitled sessions last", () => {
    const sessions = [made("zebra", 9, { title: "Zebra" }), made("none", 13), made("blank", 12, { title: "" }), made("eclair", 10, { title: "\u00c9clair" }), made("apple", 11, { title: "apple" })];
    expect(ids(arranged(sessions, "name").loose)).toEqual(["apple", "eclair", "zebra", "none", "blank"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R09
  it("breaks a tie by date created, newest first, in every sort", () => {
    const at = "2026-11-01T14:00:00.000Z";
    expect(ids(arranged([made("old", 8, { last_activity_at: at }), made("new", 9, { last_activity_at: at })], "last_activity").loose)).toEqual(["new", "old"]);
    expect(ids(arranged([made("old", 8, { title: "Same" }), made("new", 9, { title: "Same" })], "name").loose)).toEqual(["new", "old"]);
    expect(ids(arranged([made("old", 8), made("new", 9)], "name").loose)).toEqual(["new", "old"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R09
  it("sorts within each group, the Pinned group too, and leaves the groups' order alone", () => {
    const sessions = [
      made("e-b", 9, { title: "B" }, "w2"),
      made("e-a", 8, { title: "A" }, "w2"),
      made("r-b", 11, { title: "B" }),
      made("r-a", 10, { title: "A" }),
      made("p-b", 13, { title: "B", pinned: true }),
      made("p-a", 12, { title: "A", pinned: true }),
    ];
    const tree = arranged(sessions, "name", "project");
    expect(ids(tree.pinned)).toEqual(["p-a", "p-b"]);
    expect(byGroup(tree)).toEqual([
      ["ersa", ["r-a", "r-b"]],
      ["engine", ["e-a", "e-b"]],
    ]);
    expect(byGroup(arranged(sessions.filter((s) => !s.pinned), "name", "date"))).toEqual([["today", ["r-a", "e-a", "r-b", "e-b"]]]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R10
  it("keeps a tile's place through everything but a new last activity", () => {
    const quiet = made("quiet", 9, { last_activity_at: "2026-11-01T14:00:00.000Z" });
    const busy = made("busy", 10, { last_activity_at: "2026-11-01T13:00:00.000Z" });
    const before = ids(arranged([quiet, busy], "last_activity").loose);
    expect(before).toEqual(["quiet", "busy"]);
    const streaming: Session = { ...busy, state: "running", last_event_seq: 40, tile: { state: "running", pending: [{ kind: "permission", request_id: "r" }], cost_usd_total: 1 } };
    expect(ids(arranged([quiet, streaming], "last_activity").loose)).toEqual(before);
    const ended = { ...streaming, state: "idle" as const, last_activity_at: "2026-11-01T14:30:00.000Z" };
    expect(ids(arranged([quiet, ended], "last_activity").loose)).toEqual(["busy", "quiet"]);
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
