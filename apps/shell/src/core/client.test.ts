import { describe, expect, it } from "vitest";
import { describeError } from "./client";

const rpc = (message: string, detail?: string) => ({
  kind: "rpc" as const,
  error: { code: -32000, message, data: { kind: "invalid_state" as const, ...(detail === undefined ? {} : { detail }) } },
});

describe("describeError", () => {
  it("drops the method Core names first in its own message", () => {
    expect(describeError(rpc("session.send: the session is a replay of a recording and cannot change"))).toBe(
      "the session is a replay of a recording and cannot change",
    );
  });

  it("shows a detail as it is, file names and all", () => {
    expect(describeError(rpc("project.add: refused", "notes.md: no such file"))).toBe("notes.md: no such file");
  });
});
