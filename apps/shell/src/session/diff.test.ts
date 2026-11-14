// A diff card's change from a call's `edits` (issue #333, Test seams): one line diff per file, its edits applied in
// order, a file written whole as all added lines, and the counts on the row.

import { describe, expect, it } from "vitest";
import type { FileEdit } from "../protocol/agent-events.gen";
import { diffCounts, fileDiffs } from "./diff";

/** A diff's lines as "+text", "-text" or " text", to read at a glance. */
const shown = (edits: FileEdit[]) => fileDiffs(edits).map((d) => ({ path: d.path, lines: d.lines.map((l) => `${l.sign}${l.text}`) }));

describe("a file's diff", () => {
  // verifies: FLOW-READ-TRANSCRIPT-R09
  it("shows the lines an edit removes and adds, and keeps the lines it does not change", () => {
    expect(shown([{ path: "a.ts", old_text: "one\ntwo\nthree", new_text: "one\n2\nthree" }])).toEqual([
      { path: "a.ts", lines: [" one", "-two", "+2", " three"] },
    ]);
  });

  // verifies: FLOW-READ-TRANSCRIPT-R09
  it("shows a file written whole as all added lines", () => {
    expect(shown([{ path: "new.md", new_text: "# Title\n\nBody\n" }])).toEqual([{ path: "new.md", lines: ["+# Title", "+", "+Body"] }]);
  });

  // verifies: FLOW-READ-TRANSCRIPT-R09
  it("applies a file's edits in order, so an edit of what an earlier one wrote shows the change they make together", () => {
    const first: FileEdit = { path: "a.ts", old_text: "let a = 1;", new_text: "let b = 1;" };
    const second: FileEdit = { path: "a.ts", old_text: "let b = 1;", new_text: "let c = 2;" };
    expect(shown([first, second])).toEqual([{ path: "a.ts", lines: ["-let a = 1;", "+let c = 2;"] }]);
    // Applied the other way round, the second edit finds nothing to change and shows on its own.
    expect(shown([second, first])).toEqual([{ path: "a.ts", lines: ["-let b = 1;", "+let c = 2;", "-let a = 1;", "+let b = 1;"] }]);
  });

  // verifies: FLOW-READ-TRANSCRIPT-R09
  it("shows unrelated edits to one file one after another, in the order the call makes them", () => {
    const edits: FileEdit[] = [
      { path: "a.ts", old_text: "first", new_text: "First" },
      { path: "a.ts", old_text: "second", new_text: "Second" },
    ];
    expect(shown(edits)).toEqual([{ path: "a.ts", lines: ["-first", "+First", "-second", "+Second"] }]);
  });

  // verifies: FLOW-READ-TRANSCRIPT-R09
  it("gives each file its own diff, in the order the call first changes it", () => {
    const edits: FileEdit[] = [
      { path: "b.ts", old_text: "x", new_text: "y" },
      { path: "a.ts", new_text: "z" },
      { path: "b.ts", old_text: "p", new_text: "q" },
    ];
    expect(shown(edits)).toEqual([
      { path: "b.ts", lines: ["-x", "+y", "-p", "+q"] },
      { path: "a.ts", lines: ["+z"] },
    ]);
  });

  // verifies: FLOW-READ-TRANSCRIPT-R09
  it("reads Windows line endings as line breaks", () => {
    expect(shown([{ path: "a.ts", old_text: "one\r\ntwo\r\n", new_text: "one\r\n2\r\n" }])).toEqual([{ path: "a.ts", lines: [" one", "-two", "+2"] }]);
  });

  // verifies: FLOW-READ-TRANSCRIPT-R13
  it("says which files carry an edit made at every occurrence of its text", () => {
    const diffs = fileDiffs([
      { path: "a.ts", old_text: "foo", new_text: "bar", every_occurrence: true },
      { path: "b.ts", old_text: "foo", new_text: "bar" },
    ]);
    expect(diffs.map((d) => [d.path, d.everyOccurrence])).toEqual([
      ["a.ts", true],
      ["b.ts", false],
    ]);
  });
});

describe("a diff card's counts", () => {
  // verifies: FLOW-READ-TRANSCRIPT-R09
  it("are the lines added and removed across every file", () => {
    const diffs = fileDiffs([
      { path: "a.ts", old_text: "one\ntwo\nthree", new_text: "one\n2\n3\nfour\nthree" },
      { path: "b.ts", new_text: "x\ny" },
    ]);
    expect(diffCounts(diffs)).toEqual({ added: 5, removed: 1 });
  });
});
