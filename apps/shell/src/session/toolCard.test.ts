// A tool card's row from a sequence of events (issue #327, Test seams): its state, its duration or progress, and its
// target, a path in the workspace shown relative to it whatever separators either uses, and only a target whose kind
// is a path shown so (issue #368).

import { describe, expect, it } from "vitest";
import type { Envelope, Event } from "../protocol/agent-events.gen";
import { formatDuration, shownTarget, toolDetail, toolState } from "./toolCard";
import { EVENT_VERSION, type Item, type Transcript, apply, emptyTranscript } from "./transcript";

type Tool = Extract<Item, { kind: "tool" }>;

/** Events as Core sends them, each at its own time, from the live stream unless said otherwise. */
function play(steps: [string, Event][], input: "live" | "record" = "live"): Transcript {
  return steps
    .map(
      ([ts, event], seq): Envelope => ({
        v: EVENT_VERSION,
        session_id: "s1",
        seq,
        ts,
        source: { backend: "claude-code", backend_version: "2.1.316", input },
        event,
      }),
    )
    .reduce(apply, emptyTranscript);
}

const tool = (t: Transcript, id = "t1"): Tool => {
  const item = t.items.find((i) => i.key === `tool:${id}`);
  if (item?.kind !== "tool") throw new Error(`no tool card ${id}`);
  return item;
};

const requested: Event = { type: "tool.requested", tool_use_id: "t1", name: "Bash", input: { command: "cargo test" }, target: "cargo test", target_kind: "command" };
const at = (seconds: number) => new Date(Date.UTC(2026, 10, 16, 9, 0, seconds)).toISOString();
const ms = (seconds: number) => Date.parse(at(seconds));

describe("a tool card's state", () => {
  // verifies: FLOW-READ-TRANSCRIPT-R06
  it("is running, waiting for the user, done or failed", () => {
    const running = play([[at(0), requested]]);
    expect(toolState(tool(running), false)).toBe("running");
    expect(toolState(tool(running), true)).toBe("waiting");
    const done = play([[at(0), requested], [at(2), { type: "tool.result", tool_use_id: "t1", content: "ok", is_error: false }]]);
    expect(toolState(tool(done), false)).toBe("done");
    const failed = play([[at(0), requested], [at(2), { type: "tool.result", tool_use_id: "t1", content: "boom", is_error: true }]]);
    expect(toolState(tool(failed), false)).toBe("failed");
  });

  // verifies: FLOW-READ-TRANSCRIPT-R16
  it("is stopped, with no clock, when its turn ends without its result", () => {
    const t = play([
      [at(0), requested],
      [at(4), { type: "turn.completed", turn: 1, outcome: "interrupted", duration_ms: 4000, is_error: false }],
    ]);
    expect(toolState(tool(t), false)).toBe("stopped");
    // A permission request left open does not keep it waiting: nothing will come of it now.
    expect(toolState(tool(t), true)).toBe("stopped");
    expect(toolDetail(tool(t), ms(185))).toBeUndefined();
  });

  // verifies: FLOW-READ-TRANSCRIPT-R16
  it("is stopped when its session ends without its result", () => {
    const t = play([
      [at(0), requested],
      [at(4), { type: "session.ended", reason: "backend_exited" }],
    ]);
    expect(toolState(tool(t), false)).toBe("stopped");
  });

  // verifies: FLOW-READ-TRANSCRIPT-R16
  it("is not stopped by the end of a turn it finished in", () => {
    const t = play([
      [at(0), requested],
      [at(2), { type: "tool.result", tool_use_id: "t1", content: "ok", is_error: false }],
      [at(4), { type: "turn.completed", turn: 1, outcome: "success", duration_ms: 4000, is_error: false }],
    ]);
    expect(toolState(tool(t), false)).toBe("done");
    expect(tool(t).stopped).toBeUndefined();
  });
});

describe("a tool card's detail", () => {
  // verifies: FLOW-READ-TRANSCRIPT-R06
  it("is tool.result's duration_ms when the event carries one", () => {
    const t = play([[at(0), requested], [at(9), { type: "tool.result", tool_use_id: "t1", content: "", is_error: false, duration_ms: 3805 }]]);
    expect(toolDetail(tool(t), ms(60))).toBe("3s");
  });

  // verifies: FLOW-READ-TRANSCRIPT-R06
  it("is the time from its request to its result by their events' ts, without duration_ms", () => {
    const t = play([[at(0), requested], [at(42), { type: "tool.result", tool_use_id: "t1", content: "", is_error: false }]]);
    expect(toolDetail(tool(t), ms(100))).toBe("42s");
  });

  // verifies: FLOW-READ-TRANSCRIPT-R06
  it("is its latest progress while it runs live, else its elapsed time", () => {
    const started = play([[at(0), requested]]);
    expect(toolDetail(tool(started), ms(185))).toBe("3m 5s");
    const progressing = play([
      [at(0), requested],
      [at(1), { type: "tool.progress", tool_use_id: "t1", detail: "Compiling" }],
      [at(2), { type: "tool.progress", tool_use_id: "t1", detail: "Running 12 tests" }],
    ]);
    expect(toolDetail(tool(progressing), ms(185))).toBe("Running 12 tests");
  });

  // verifies: FLOW-READ-TRANSCRIPT-R06
  it("shows no clock for a call read from the record, and its duration once it has one", () => {
    const unfinished = play([[at(0), requested]], "record");
    expect(toolDetail(tool(unfinished), ms(185))).toBeUndefined();
    const finished = play([[at(0), requested], [at(7), { type: "tool.result", tool_use_id: "t1", content: "", is_error: false }]], "record");
    expect(toolDetail(tool(finished), ms(185))).toBe("7s");
  });

  // verifies: FLOW-READ-TRANSCRIPT-R06
  it("stops its clock once the session has ended", () => {
    const t = play([
      [at(0), requested],
      [at(4), { type: "session.ended", reason: "user_stopped" }],
    ]);
    expect(toolDetail(tool(t), ms(185))).toBeUndefined();
  });

  it("shows from 1s", () => {
    const t = play([[at(0), requested], [at(0), { type: "tool.result", tool_use_id: "t1", content: "", is_error: false, duration_ms: 999 }]]);
    expect(toolDetail(tool(t), ms(5))).toBeUndefined();
    expect(toolDetail(tool(play([[at(0), requested]])), ms(0) + 400)).toBeUndefined();
  });
});

describe("durations, as CONTENT.md writes them", () => {
  it("uses the two largest units, abbreviated and without a space", () => {
    expect(formatDuration(1000)).toBe("1s");
    expect(formatDuration(42_900)).toBe("42s");
    expect(formatDuration(185_000)).toBe("3m 5s");
    expect(formatDuration(180_000)).toBe("3m");
    expect(formatDuration(4_320_000)).toBe("1h 12m");
    expect(formatDuration(3_600_000)).toBe("1h");
  });
});

describe("a tool card's target", () => {
  it("is the event's target and its kind, carried on the card", () => {
    expect(tool(play([[at(0), requested]])).target).toBe("cargo test");
    expect(tool(play([[at(0), requested]])).targetKind).toBe("command");
    const bare = play([[at(0), { type: "tool.requested", tool_use_id: "t1", name: "Mcp", input: {} }]]);
    expect(tool(bare).target).toBeUndefined();
    expect(tool(bare).targetKind).toBeUndefined();
  });

  // verifies: FLOW-READ-TRANSCRIPT-R08
  it("shows a path inside the workspace relative to it, whatever separators each uses", () => {
    expect(shownTarget("D:\\Projects\\ersa\\docs\\GLOSSARY.md", "path", "D:\\Projects\\ersa")).toEqual({
      text: "docs\\GLOSSARY.md",
      path: true,
    });
    expect(shownTarget("D:/Projects/ersa/docs/GLOSSARY.md", "path", "D:\\Projects\\ersa\\")).toEqual({
      text: "docs/GLOSSARY.md",
      path: true,
    });
    expect(shownTarget("d:\\projects\\Ersa\\a.txt", "path", "D:/Projects/ersa")).toEqual({ text: "a.txt", path: true });
    expect(shownTarget("/home/me/ersa/src/main.rs", "path", "/home/me/ersa")).toEqual({ text: "src/main.rs", path: true });
    // A file whose name has a space, in Windows and POSIX folders.
    expect(shownTarget("D:\\Projects\\ersa\\docs\\Meeting notes.md", "path", "D:\\Projects\\ersa")).toEqual({ text: "docs\\Meeting notes.md", path: true });
    expect(shownTarget("/home/me/ersa/docs/Meeting notes.md", "path", "/home/me/ersa")).toEqual({ text: "docs/Meeting notes.md", path: true });
    // A folder with a space in its name.
    expect(shownTarget("/Users/jane/My Projects/app/src/main.rs", "path", "/Users/jane/My Projects/app")).toEqual({ text: "src/main.rs", path: true });
  });

  // verifies: FLOW-READ-TRANSCRIPT-R08
  it("shows a path outside the workspace, or one only sharing its prefix, as it is, still a path", () => {
    expect(shownTarget("D:\\Projects\\ersa-old\\a.txt", "path", "D:\\Projects\\ersa")).toEqual({
      text: "D:\\Projects\\ersa-old\\a.txt",
      path: true,
    });
    expect(shownTarget("C:\\Windows\\a.txt", "path", "D:\\Projects\\ersa")).toEqual({ text: "C:\\Windows\\a.txt", path: true });
    expect(shownTarget("D:\\Projects\\ersa", "path", "D:\\Projects\\ersa")).toEqual({ text: "D:\\Projects\\ersa", path: true });
    expect(shownTarget("D:\\Projects\\ersa\\a.txt", "path", undefined)).toEqual({ text: "D:\\Projects\\ersa\\a.txt", path: true });
    // A relative path is a path by its kind, whatever its spelling.
    expect(shownTarget("notes", "path", "D:\\Projects\\ersa")).toEqual({ text: "notes", path: true });
  });

  // verifies: FLOW-READ-TRANSCRIPT-R08
  it("takes a target for a path by its kind alone, never by its spelling", () => {
    // A command that begins with a path in the workspace stays whole and is cut at its end.
    expect(shownTarget("D:\\Projects\\ersa\\build.cmd --release", "command", "D:\\Projects\\ersa")).toEqual({
      text: "D:\\Projects\\ersa\\build.cmd --release",
      path: false,
    });
    expect(shownTarget("/home/me/ersa/run.sh", "command", "/home/me/ersa")).toEqual({ text: "/home/me/ersa/run.sh", path: false });
    // A search spelled like an absolute path is not made relative.
    expect(shownTarget("D:\\Projects\\ersa\\src", "search", "D:\\Projects\\ersa")).toEqual({ text: "D:\\Projects\\ersa\\src", path: false });
    expect(shownTarget("/home/me/ersa/**/*.md", "search", "/home/me/ersa")).toEqual({ text: "/home/me/ersa/**/*.md", path: false });
    expect(shownTarget("https://example.com/a", "address", "D:\\Projects\\ersa")).toEqual({ text: "https://example.com/a", path: false });
    expect(shownTarget("Count files", "task", "D:\\Projects\\ersa")).toEqual({ text: "Count files", path: false });
    // A target whose event names no kind is no path.
    expect(shownTarget("C:\\Windows\\a.txt", undefined, "D:\\Projects\\ersa")).toEqual({ text: "C:\\Windows\\a.txt", path: false });
  });

  // verifies: FLOW-READ-TRANSCRIPT-R08
  it("compares a Windows path without case, and any other path with it", () => {
    expect(shownTarget("\\\\server\\share\\Ersa\\a.txt", "path", "\\\\server\\share\\ersa")).toEqual({ text: "a.txt", path: true });
    expect(shownTarget("/home/Me/ersa/a.txt", "path", "/home/me/ersa")).toEqual({ text: "/home/Me/ersa/a.txt", path: true });
  });
});

describe("a request seen again", () => {
  it("keeps what happened since it was first asked for", () => {
    const t = play([
      [at(0), requested],
      [at(5), { type: "tool.result", tool_use_id: "t1", content: "ok", is_error: false }],
      [at(6), { ...requested, target: "cargo test --all" } as Event],
    ]);
    expect(tool(t)).toMatchObject({ target: "cargo test --all", requestedAt: at(0), result: { content: "ok", at: at(5) } });
    expect(toolDetail(tool(t), ms(100))).toBe("5s");
  });

  // verifies: FLOW-READ-TRANSCRIPT-R09
  it("keeps the edits its event carries, for its diff card", () => {
    const edit: Event = {
      type: "tool.requested",
      tool_use_id: "e1",
      name: "Edit",
      input: {},
      target: "a.ts",
      edits: [{ path: "a.ts", old_text: "a", new_text: "b" }],
    };
    expect(tool(play([[at(0), edit]]), "e1").edits).toEqual([{ path: "a.ts", old_text: "a", new_text: "b" }]);
  });
});

describe("a subagent's calls", () => {
  // verifies: FLOW-READ-TRANSCRIPT-R10
  it("carry the call that started the subagent", () => {
    const t = play([
      [at(0), { type: "tool.requested", tool_use_id: "agent", name: "Agent", input: { description: "Count files" }, target: "Count files" }],
      [at(1), { type: "tool.requested", tool_use_id: "glob", name: "Glob", input: { pattern: "*" }, parent_tool_use_id: "agent", target: "*" }],
    ]);
    expect(tool(t, "agent").parent).toBeUndefined();
    expect(tool(t, "glob").parent).toBe("tool:agent");
  });
});
