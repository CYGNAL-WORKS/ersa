// A diff card's change (FLOW-READ-TRANSCRIPT-R09, R13; docs/design/TRANSCRIPT.md, Diff cards): one line diff per
// file from the call's `edits`, as PROTO-AGENT-EVENTS carries them. Kept free of React so tests call it.

import type { FileEdit } from "../protocol/agent-events.gen";

/** One line of a diff: added, removed, or kept as it was. */
export interface DiffLine {
  sign: "+" | "-" | " ";
  text: string;
}

/** One file's diff, and whether an edit to it is made at every occurrence of its text (R13). */
export interface FileDiff {
  path: string;
  lines: DiffLine[];
  everyOccurrence: boolean;
}

/** A stretch of the file a call changes: the text that was there and the text the call leaves. */
interface Change {
  old: string;
  new: string;
}

/**
 * One diff per file, in the order the call first changes each, its edits applied in order (R09). The call carries the
 * text it replaces, not the whole file, so a file's diff is built from its changes:
 * - an edit with no `old_text` writes the file whole: everything before it is replaced, and it shows as added lines;
 * - an edit whose `old_text` lies in what an earlier edit wrote changes that text, so the two show as one change;
 * - any other edit is a change of its own, shown after the ones before it.
 */
export function fileDiffs(edits: readonly FileEdit[]): FileDiff[] {
  const files = new Map<string, { changes: Change[]; everyOccurrence: boolean }>();
  for (const edit of edits) {
    let file = files.get(edit.path);
    if (!file) {
      file = { changes: [], everyOccurrence: false };
      files.set(edit.path, file);
    }
    if (edit.every_occurrence) file.everyOccurrence = true;
    if (edit.old_text === undefined) {
      file.changes = [{ old: "", new: edit.new_text }];
      continue;
    }
    const old = edit.old_text;
    const earlier = old === "" ? undefined : file.changes.find((c) => c.new.includes(old));
    if (earlier) {
      // A function as the replacement, so a `$` in the new text is taken as itself.
      earlier.new = edit.every_occurrence ? earlier.new.split(old).join(edit.new_text) : earlier.new.replace(old, () => edit.new_text);
    } else {
      file.changes.push({ old, new: edit.new_text });
    }
  }
  return [...files].map(([path, file]) => ({
    path,
    lines: file.changes.flatMap((c) => lineDiff(lines(c.old), lines(c.new))),
    everyOccurrence: file.everyOccurrence,
  }));
}

/** The lines added and removed across every file, for the card's row. */
export function diffCounts(diffs: readonly FileDiff[]): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const diff of diffs) {
    for (const line of diff.lines) {
      if (line.sign === "+") added++;
      else if (line.sign === "-") removed++;
    }
  }
  return { added, removed };
}

/** Text as lines, `\n` and `\r\n` alike; a final line break ends the last line rather than starting an empty one. */
function lines(text: string): string[] {
  if (text === "") return [];
  const all = text.split(/\r?\n/);
  if (all[all.length - 1] === "") all.pop();
  return all;
}

/**
 * Past this many pairs of lines to compare, a change shows as its old lines removed and its new lines added: comparing
 * every pair of a long file's lines would hold up the transcript for a diff that reads no better.
 */
const MAX_PAIRS = 4_000_000;

/**
 * The shortest line diff from `a` to `b`, by their longest common run of lines; where both have lines in its place,
 * the removed ones come before the added ones, as a reader expects a replacement.
 */
function lineDiff(a: string[], b: string[]): DiffLine[] {
  if (a.length * b.length > MAX_PAIRS) {
    return [...a.map((text): DiffLine => ({ sign: "-", text })), ...b.map((text): DiffLine => ({ sign: "+", text }))];
  }
  // common[i * width + j]: how many lines a[i..] and b[j..] have in common, in order; one flat table, row by row.
  const width = b.length + 1;
  const common = new Uint32Array((a.length + 1) * width);
  const at = (i: number, j: number) => common[i * width + j] ?? 0;
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      common[i * width + j] = a[i] === b[j] ? at(i + 1, j + 1) + 1 : Math.max(at(i + 1, j), at(i, j + 1));
    }
  }
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    const old = a[i];
    const added = b[j];
    if (old !== undefined && added !== undefined && old === added) {
      out.push({ sign: " ", text: old });
      i++;
      j++;
    } else if (old !== undefined && (added === undefined || at(i + 1, j) >= at(i, j + 1))) {
      out.push({ sign: "-", text: old });
      i++;
    } else if (added !== undefined) {
      out.push({ sign: "+", text: added });
      j++;
    }
  }
  return out;
}
