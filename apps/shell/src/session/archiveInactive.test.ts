import { describe, expect, it } from "vitest";
import { ARCHIVE_DAYS, archiveDaysOf, archivePatch } from "./archiveInactive";
import { mergePatch } from "./arrangement";

describe("Archive inactive sessions as Core's settings keep it", () => {
  // verifies: FLOW-CLEAN-UP-R15
  it("offers Never and 1, 2, 7, 14 or 30 days", () => {
    expect(ARCHIVE_DAYS).toEqual([1, 2, 7, 14, 30]);
  });

  // verifies: FLOW-CLEAN-UP-R15
  it("reads the value in force from `sessions.archive_inactive_days`, and Never when it is unset", () => {
    for (const days of [1, 2, 7, 14, 30]) expect(archiveDaysOf({ sessions: { archive_inactive_days: days } })).toBe(days);
    expect(archiveDaysOf({})).toBeUndefined();
    expect(archiveDaysOf({ sessions: {} })).toBeUndefined();
  });

  it("reads anything Core would refuse as Never", () => {
    for (const odd of [3, 0, -1, "7", null, true, [7]]) expect(archiveDaysOf({ sessions: { archive_inactive_days: odd } })).toBeUndefined();
    expect(archiveDaysOf(null)).toBeUndefined();
    expect(archiveDaysOf({ sessions: 7 })).toBeUndefined();
  });

  // verifies: FLOW-CLEAN-UP-R15
  it("writes a choice to `sessions.archive_inactive_days`, and Never clears it", () => {
    expect(archivePatch(14)).toEqual({ sessions: { archive_inactive_days: 14 } });
    expect(archivePatch(undefined)).toEqual({ sessions: { archive_inactive_days: null } });
    // Merged as Core merges a patch (RFC 7386), Never leaves the key unset and other settings as they were.
    const kept = { sessions: { archive_inactive_days: 7 }, rail: { group_by: "date" } };
    expect(mergePatch(kept, archivePatch(undefined))).toEqual({ sessions: {}, rail: { group_by: "date" } });
  });
});
