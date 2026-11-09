import { describe, expect, it } from "vitest";
import { DEFAULT_ARRANGEMENT, arrangementOf, collapsePatch, mergePatch, RESET_PATCH, choicePatch } from "./arrangement";

describe("the rail's arrangement in Core's settings", () => {
  it("reads the defaults from settings that hold none of the rail's keys", () => {
    expect(arrangementOf({})).toEqual(DEFAULT_ARRANGEMENT);
    expect(arrangementOf(undefined)).toEqual(DEFAULT_ARRANGEMENT);
    expect(DEFAULT_ARRANGEMENT).toEqual({ groupBy: "project", show: "active", collapsed: {} });
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R13
  it("reads back the grouping, the filter and each grouping's collapsed groups it saved, as a restart does", () => {
    let settings: unknown = { new_session: { last_used: {} } };
    for (const patch of [choicePatch({ groupBy: "date" }), choicePatch({ show: "all" }), collapsePatch("date", ["today", "pinned"]), collapsePatch("project", ["p1"])])
      settings = mergePatch(settings, patch);
    expect(settings).toEqual({
      new_session: { last_used: {} },
      rail: { group_by: "date", show: "all", collapsed: { date: ["today", "pinned"], project: ["p1"] } },
    });
    expect(arrangementOf(settings)).toEqual({ groupBy: "date", show: "all", collapsed: { date: ["today", "pinned"], project: ["p1"] } });
  });

  it("reads a value it does not know as the default, since settings are the user's and may hold anything", () => {
    const odd = { rail: { group_by: "section", show: 3, collapsed: { project: ["p1", 7], date: "today", none: null } } };
    expect(arrangementOf(odd)).toEqual({ groupBy: "project", show: "active", collapsed: { project: ["p1"] } });
    expect(arrangementOf({ rail: "flat" })).toEqual(DEFAULT_ARRANGEMENT);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R14
  it("resets the grouping, the sort, the filter and every collapsed group, and nothing outside the rail's keys", () => {
    expect(RESET_PATCH).toEqual({ rail: { group_by: null, sort_by: null, show: null, collapsed: null } });
    const settings = {
      new_session: { last_used: { p1: { host_id: "local" } } },
      rail: { group_by: "none", sort_by: "name", show: "archived", collapsed: { none: ["pinned"] } },
    };
    const reset = mergePatch(settings, RESET_PATCH);
    expect(reset).toEqual({ new_session: { last_used: { p1: { host_id: "local" } } }, rail: {} });
    expect(arrangementOf(reset)).toEqual({ groupBy: "project", show: "active", collapsed: {} });
  });

  it("merges a patch as RFC 7386 does: objects merge, arrays and values replace, null removes", () => {
    expect(mergePatch({ a: { b: 1, c: [1, 2] } }, { a: { c: [3], d: null } })).toEqual({ a: { b: 1, c: [3] } });
    expect(mergePatch("text", { a: 1 })).toEqual({ a: 1 });
    expect(mergePatch({ a: 1 }, null)).toBeNull();
  });
});
