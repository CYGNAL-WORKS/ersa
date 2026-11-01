import { describe, expect, it } from "vitest";
import type { Session } from "../protocol/core-api.gen";
import { newestFirst } from "./SessionList";

const at = (id: string, created_at: string): Session => ({
  id,
  workspace_id: "w",
  host_id: "local",
  backend: "claude-code",
  state: "idle",
  created_at,
  tile: { state: "idle", pending: [] },
});

describe("newestFirst", () => {
  it("orders by creation time, newest first, without changing its input", () => {
    const sessions = [at("a", "2026-11-01T09:00:00.000Z"), at("b", "2026-11-01T10:00:00.000Z"), at("c", "2026-11-01T09:30:00.000Z")];
    expect(newestFirst(sessions).map((s) => s.id)).toEqual(["b", "c", "a"]);
    expect(sessions.map((s) => s.id)).toEqual(["a", "b", "c"]);
  });
});
