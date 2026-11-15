// The session controls' ring reads the nearest limit (LAY-SHELL, Context; docs/design/COMPOSER.md, The context
// ring; issue #422). A layout brief, so no requirement IDs: the tests cite the sections.

import { describe, expect, it } from "vitest";
import type { Envelope, Event, UsageUpdated } from "../protocol/agent-events.gen";
import { type UsageWindow, nearest, readings, resetsIn } from "./limits";
import { EVENT_VERSION, apply, emptyTranscript } from "./transcript";

const envelope = (seq: number, event: Event): Envelope => ({
  v: EVENT_VERSION,
  session_id: "s1",
  seq,
  ts: "2026-11-15T12:00:00Z",
  source: { backend: "claude-code", backend_version: "2.1.302", input: "live" },
  event,
});
const usage = (seq: number, fields: Omit<UsageUpdated, "type">) => envelope(seq, { type: "usage.updated", ...fields });
const fiveHour: UsageWindow = { id: "five_hour", label: "5-hour limit", used: 0.84, resets_at: "2026-11-15T13:15:00Z" };
const weekly: UsageWindow = { id: "seven_day", label: "Weekly · all models", used: 0.3 };

describe("a session's usage windows", () => {
  it("are the latest report's list, in the order reported", () => {
    const t = [usage(0, { rate_limit: { status: "allowed", windows: [weekly, fiveHour] } })].reduce(apply, emptyTranscript);
    expect(t.windows).toEqual([weekly, fiveHour]);
  });

  it("are replaced by a report's list, emptied by an empty one", () => {
    const t = [
      usage(0, { rate_limit: { status: "allowed", windows: [weekly, fiveHour] } }),
      usage(1, { rate_limit: { status: "allowed", windows: [fiveHour] } }),
    ].reduce(apply, emptyTranscript);
    expect(t.windows).toEqual([fiveHour]);
    expect(apply(t, usage(2, { rate_limit: { status: "allowed", windows: [] } })).windows).toEqual([]);
  });

  it("stand when a report has no list, or no rate limit", () => {
    const t = [
      usage(0, { rate_limit: { status: "allowed", windows: [weekly] } }),
      usage(1, { rate_limit: { status: "allowed_warning" } }),
      usage(2, { context_tokens: 10, context_window: 100 }),
    ].reduce(apply, emptyTranscript);
    expect(t.windows).toEqual([weekly]);
  });

  it("are none before any report", () => {
    expect(emptyTranscript.windows).toBeUndefined();
  });
});

describe("the readings", () => {
  it("are the context first, then each window in the order reported, never re-sorted", () => {
    const list = readings(0.3, [weekly, fiveHour]);
    expect(list.map((r) => r.label)).toEqual([undefined, "Weekly · all models", "5-hour limit"]);
    expect(list.map((r) => r.percent)).toEqual([30, 30, 84]);
  });

  it("read a window as the whole percent, rounded down, and never more than full", () => {
    const at = (used: number) => readings(undefined, [{ ...weekly, used }])[1]?.percent;
    expect(at(0.849)).toBe(84);
    expect(at(0.29)).toBe(29);
    expect(at(0.999)).toBe(99);
    expect(at(1.2)).toBe(100);
    expect(at(-0.1)).toBe(0);
  });

  it("read the context as the ring rounds it", () => {
    expect(readings(0.845, [])[0]?.percent).toBe(85);
    expect(readings(undefined, [])[0]?.percent).toBeUndefined();
  });

  it("leave out a window that isn't well formed", () => {
    const odd = [{ ...weekly, used: Number.NaN }, { ...weekly, label: 3 as unknown as string }, fiveHour];
    expect(readings(0.1, odd).map((r) => r.label)).toEqual([undefined, "5-hour limit"]);
  });
});

describe("the nearest limit", () => {
  it("is a window above the context", () => {
    // 30 percent context, a window at 84: the ring reads the window, in danger.
    const near = nearest(readings(0.3, [{ ...weekly, used: 0.2 }, fiveHour]));
    expect(near.label).toBe("5-hour limit");
    expect(near.percent).toBe(84);
  });

  it("is the context on a tie", () => {
    const near = nearest(readings(0.3, [weekly]));
    expect(near.label).toBeUndefined();
    expect(near.percent).toBe(30);
  });

  it("goes by the figure shown, so it is never smaller than another reading's figure", () => {
    // The context rounds to 85; a window at 84.9 percent reads 84.
    expect(nearest(readings(0.845, [{ ...weekly, used: 0.849 }])).label).toBeUndefined();
  });

  it("is the first window among equals", () => {
    expect(nearest(readings(0.1, [{ ...weekly, used: 0.5 }, { ...fiveHour, used: 0.5 }])).label).toBe("Weekly · all models");
  });

  it("is any window over a context not reported yet", () => {
    expect(nearest(readings(undefined, [{ ...weekly, used: 0 }])).label).toBe("Weekly · all models");
  });

  it("is the context alone with no windows", () => {
    expect(nearest(readings(0.42, undefined))).toEqual({ percent: 42 });
  });
});

describe("when a window resets", () => {
  // Vitest runs in America/New_York (vite.config.ts): 2026-11-15T12:00Z is Sunday 7:00 AM there.
  const now = new Date("2026-11-15T12:00:00Z");
  const words = (resetsAt: string | undefined) => resetsIn(resetsAt, now)?.replace(/ /g, " ");

  it("reads a weekday and time a day or more away", () => {
    // Wednesday 2:00 PM in New York.
    expect(words("2026-11-18T19:00:00Z")).toBe("Resets Wed 2:00 PM");
    expect(words("2026-11-16T12:00:00Z")).toBe("Resets Mon 7:00 AM");
  });

  it("reads in the user's own time zone", () => {
    // 01:30Z on Thursday is still Wednesday evening in New York.
    expect(words("2026-11-19T01:30:00Z")).toBe("Resets Wed 8:30 PM");
  });

  it("reads hours and minutes left within a day", () => {
    expect(words("2026-11-15T13:15:00Z")).toBe("Resets in 1 hr 15 min");
    expect(words("2026-11-16T11:59:00Z")).toBe("Resets in 23 hr 59 min");
  });

  it("reads whole hours alone", () => {
    expect(words("2026-11-15T14:00:00Z")).toBe("Resets in 2 hr");
  });

  it("reads minutes alone under an hour, rounded up, never none", () => {
    expect(words("2026-11-15T12:15:00Z")).toBe("Resets in 15 min");
    expect(words("2026-11-15T12:14:01Z")).toBe("Resets in 15 min");
    expect(words("2026-11-15T12:00:10Z")).toBe("Resets in 1 min");
    expect(words("2026-11-15T11:00:00Z")).toBe("Resets in 1 min");
  });

  it("says nothing without a time it can read", () => {
    expect(words(undefined)).toBeUndefined();
    expect(words("soon")).toBeUndefined();
  });
});
