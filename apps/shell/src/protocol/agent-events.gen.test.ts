// The Interface's side of the event schema's conformance: the generated types
// take what the schema takes and refuse what it refuses. `tsc` checks the
// `@ts-expect-error` lines; vitest only runs the file.

import { describe, expect, it } from "vitest";
import type { UsageUpdated } from "./agent-events.gen";

type Window = NonNullable<NonNullable<UsageUpdated["rate_limit"]>["windows"]>[number];

describe("usage.updated windows", () => {
  // verifies: PROTO-CLAUDE-CODE-STREAM-R76
  it("takes a window with its id, label and share, and refuses one without", () => {
    const update: UsageUpdated = {
      type: "usage.updated",
      rate_limit: {
        status: "allowed",
        windows: [
          { id: "seven_day", label: "Weekly · all models", used: 0.04, resets_at: "2026-11-04T20:00:00Z" },
          { id: "five_hour", label: "5-hour limit", used: 1.2 },
        ],
      },
    };
    // @ts-expect-error: a window needs its id.
    const noId: Window = { label: "5-hour limit", used: 0.5 };
    // @ts-expect-error: a window needs its label.
    const noLabel: Window = { id: "five_hour", used: 0.5 };
    // @ts-expect-error: a window needs its share.
    const noShare: Window = { id: "five_hour", label: "5-hour limit" };
    expect(update.rate_limit?.windows?.map((w) => w.id)).toEqual(["seven_day", "five_hour"]);
    expect([noId, noLabel, noShare]).toHaveLength(3);
  });
});
