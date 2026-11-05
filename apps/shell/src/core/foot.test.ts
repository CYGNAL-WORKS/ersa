import { describe, expect, it } from "vitest";
import type { CoreHelloResult } from "../protocol/core-api.gen";
import type { CoreView } from "./connection";
import { footOf } from "./foot";

const hello = { core_version: "0.1.0", api_version: "1", host: { name: "PC" } } as unknown as CoreHelloResult;
const connected = (label?: string): CoreView => ({
  phase: "connected",
  link: 1,
  how: "attached",
  ...(label === undefined ? {} : { label }),
  hello,
  status: { sessions_running: 3, sessions_waiting: 1, uptime_s: 42, db_path: "" },
  projects: { state: "loading" },
  sessions: { state: "loading" },
});

describe("what the rail's foot shows", () => {
  it("says nothing while first connecting", () => {
    expect(footOf({ phase: "connecting" })).toEqual({ kind: "silent" });
  });

  it("says nothing while connected to the user's own Core, whatever Core reports", () => {
    expect(footOf(connected())).toEqual({ kind: "silent" });
  });

  it("names the label of a labeled endpoint", () => {
    expect(footOf(connected("replay"))).toEqual({ kind: "attached", label: "replay" });
  });

  it("says Ersa is trying again when the connection is lost, with the reason", () => {
    expect(footOf({ phase: "lost", reason: "closed", hello })).toEqual({ kind: "reconnecting", reason: "closed" });
  });

  it("offers Retry once Ersa has stopped trying, with the reason", () => {
    expect(footOf({ phase: "failed", message: "exit code 1" })).toEqual({ kind: "unreachable", reason: "exit code 1" });
  });

  it("leaves the reason out when there is none", () => {
    expect(footOf({ phase: "lost", reason: "  ", hello })).toEqual({ kind: "reconnecting" });
    expect(footOf({ phase: "failed", message: "" })).toEqual({ kind: "unreachable" });
  });
});
