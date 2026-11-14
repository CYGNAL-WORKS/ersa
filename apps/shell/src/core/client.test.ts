import { describe, expect, it } from "vitest";
import { t } from "../catalog";
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

// A refusal Core names with a `reason`, as Core sends it: the message is Core's, for the log.
const refused = (reason: string, detail?: string) => ({
  kind: "rpc" as const,
  error: {
    code: -32000,
    message: `session.send: core's own words for ${reason}`,
    data: { kind: "invalid_state" as const, reason, ...(detail === undefined ? {} : { detail }) },
  },
});

describe("describeError over a refusal with a reason", () => {
  // Every reason the Interface knows, with the catalog's words for it. `folder_gone`'s words are #340's.
  const known = [
    ["session_busy", "errors.refusal.sessionBusyError"],
    ["session_deleting", "errors.refusal.sessionDeletingError"],
    ["session_archived", "errors.refusal.sessionArchivedError"],
    ["session_not_running", "errors.refusal.sessionNotRunningError"],
    ["send_in_progress", "errors.refusal.sendInProgressError"],
    ["send_stopped", "errors.refusal.sendStoppedError"],
    ["backend_unnamed", "errors.refusal.backendUnnamedError"],
    ["record_gone", "errors.refusal.recordGoneError"],
    ["open_unknown", "errors.refusal.openUnknownError"],
    ["no_finished_reply", "errors.refusal.noFinishedReplyError"],
    ["backend_cannot_fork", "errors.refusal.backendCannotForkError"],
  ] as const;

  // verifies: PROTO-CORE-API-R80
  it.each(known)("says %s in the catalog's words, never Core's message or detail", (reason, key) => {
    const words = describeError(refused(reason, "a detail Core added"));
    expect(words).toBe(t(key));
    expect(words).not.toContain("core's own words");
    expect(words).not.toContain("a detail Core added");
  });

  // verifies: PROTO-CORE-API-R80
  // verifies: PROTO-CORE-API-R87
  it("names the one process that has the session open, in its own words", () => {
    expect(describeError(refused("open_elsewhere", "4242"))).toBe(
      "This session is open in another program, process 4242. Close it there, then try again.",
    );
  });

  // verifies: PROTO-CORE-API-R87
  it("names every process that has the session open, as Core lists them", () => {
    expect(describeError(refused("open_elsewhere", "4242, 4343"))).toBe(
      "This session is open in other programs, processes 4242, 4343. Close them there, then try again.",
    );
  });

  // verifies: PROTO-CORE-API-R87
  it("shows no detail that isn't a list of process ids", () => {
    const words = describeError(refused("open_elsewhere", "4242; rm -rf"));
    expect(words).toBe(t("errors.refusal.openElsewhereUnnamedError"));
    expect(words).not.toContain("4242");
  });

  // verifies: PROTO-CORE-API-R88
  it("shows the adapter's message for a send that could not start its backend, which has no reason", () => {
    const error = {
      kind: "rpc" as const,
      error: {
        code: -32000,
        message: "session.send: the backend could not be started: Claude Code is not signed in",
        data: { kind: "invalid_state" as const },
      },
    };
    expect(describeError(error)).toBe("the backend could not be started: Claude Code is not signed in");
    expect(describeError(error)).not.toBe(t("errors.refusal.sessionNotRunningError"));
  });

  it("falls back to Core's message for a reason this build doesn't know", () => {
    expect(describeError(refused("something_newer"))).toBe("core's own words for something_newer");
  });
});
