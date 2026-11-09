import { describe, expect, it } from "vitest";
import { lastActive, pageRows } from "./sessionsPage";

const row = (id: string, more: Partial<Parameters<typeof pageRows>[0][number]> = {}) => ({
  id,
  title: id,
  archived: false,
  created_at: "2026-11-01T09:00:00Z",
  ...more,
});
const ids = (rows: { id: string }[]) => rows.map((r) => r.id);

describe("the sessions page's rows", () => {
  // verifies: FLOW-ORGANIZE-SESSIONS-R16
  it("lists each tab's sessions, newest last activity first, taking creation for a session with no activity yet", () => {
    const sessions = [
      row("old-activity", { created_at: "2026-11-05T09:00:00Z", last_activity_at: "2026-11-02T09:00:00Z" }),
      row("no-activity", { created_at: "2026-11-03T09:00:00Z" }),
      row("new-activity", { created_at: "2026-10-01T09:00:00Z", last_activity_at: "2026-11-04T09:00:00.500Z" }),
      row("archived", { archived: true, created_at: "2026-11-06T09:00:00Z" }),
    ];
    expect(ids(pageRows(sessions, "active", ""))).toEqual(["new-activity", "no-activity", "old-activity"]);
    expect(ids(pageRows(sessions, "archived", ""))).toEqual(["archived"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R16
  it("orders timestamps by time, not by how many decimals they carry", () => {
    const sessions = [row("whole", { created_at: "2026-11-01T09:00:00Z" }), row("later", { last_activity_at: "2026-11-01T09:00:00.500Z" })];
    expect(ids(pageRows(sessions, "active", ""))).toEqual(["later", "whole"]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R17
  it("keeps only the titles containing the search text, ignoring case", () => {
    const sessions = [row("a", { title: "Fix the BUILD" }), row("b", { title: "Write notes" }), row("c", { title: "rebuild docs" })];
    expect(ids(pageRows(sessions, "active", "build")).sort()).toEqual(["a", "c"]);
    expect(ids(pageRows(sessions, "active", "BUILD")).sort()).toEqual(["a", "c"]);
    expect(pageRows(sessions, "active", "nothing like it")).toEqual([]);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R17
  it("matches an untitled session by the title the page shows for it", () => {
    expect(ids(pageRows([row("u", { title: undefined })], "active", "untitled"))).toEqual(["u"]);
  });
});

describe("when a row says its session was last active", () => {
  const now = new Date(2026, 10, 7, 15, 0, 0);
  const at = (d: Date) => ({ created_at: "2020-01-01T00:00:00Z", last_activity_at: d.toISOString() });

  it("words it by how long ago it was", () => {
    expect(lastActive(at(new Date(2026, 10, 7, 14, 59, 30)), now)).toBe("Just now");
    expect(lastActive(at(new Date(2026, 10, 7, 14, 59, 0)), now)).toBe("1 minute ago");
    expect(lastActive(at(new Date(2026, 10, 7, 14, 55, 0)), now)).toBe("5 minutes ago");
    expect(lastActive(at(new Date(2026, 10, 7, 14, 0, 0)), now)).toBe("1 hour ago");
    expect(lastActive(at(new Date(2026, 10, 7, 9, 0, 0)), now)).toBe("6 hours ago");
    expect(lastActive(at(new Date(2026, 10, 6, 1, 0, 0)), now)).toBe("Yesterday");
    expect(lastActive(at(new Date(2026, 10, 5, 23, 0, 0)), now)).toBe("Nov 5, 2026");
  });

  it("counts yesterday by the calendar, so soon after midnight an hour before it is minutes, then yesterday", () => {
    const midnight = new Date(2026, 10, 7, 0, 30, 0);
    expect(lastActive(at(new Date(2026, 10, 6, 23, 50, 0)), midnight)).toBe("40 minutes ago");
    expect(lastActive(at(new Date(2026, 10, 6, 22, 0, 0)), midnight)).toBe("Yesterday");
  });

  it("reads a time ahead of this machine's clock as now, and says nothing of one it can't read", () => {
    expect(lastActive(at(new Date(2026, 10, 7, 15, 5, 0)), now)).toBe("Just now");
    expect(lastActive({ created_at: "not a date" }, now)).toBe("");
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R16
  it("takes the creation until the session has activity", () => {
    expect(lastActive({ created_at: new Date(2026, 10, 7, 14, 55, 0).toISOString() }, now)).toBe("5 minutes ago");
  });
});
