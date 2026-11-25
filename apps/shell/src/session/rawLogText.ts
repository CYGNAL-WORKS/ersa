// What the developer pane does with a raw log line's text (FLOW-READ-RAW-LOG, steps 4 and 5): lays out a line of JSON
// with indentation for its detail, matches lines against the filter, and measures a line. Kept free of React.

import type { RawLogEntry } from "../protocol/core-api.gen";

/** Two spaces a level. */
const INDENT = "  ";

/**
 * A whole line of JSON laid out with indentation (FLOW-READ-RAW-LOG-R10); undefined when the line is not valid JSON,
 * or is a lone value with nothing to indent. Only whitespace between tokens changes: every number, string and escape
 * stays exactly as it crossed, which parsing and printing again would not keep (1.0 would read 1, a large integer
 * would be rounded, é would become é).
 */
export function indentJson(line: string): string | undefined {
  try {
    const value: unknown = JSON.parse(line);
    if (value === null || typeof value !== "object") return undefined;
  } catch {
    return undefined;
  }
  // The line is valid JSON, so a plain scan over it is safe: strings are copied whole, and outside them whitespace is
  // dropped and laid out again around brackets, commas and colons.
  let out = "";
  let depth = 0;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i]!;
    if (ch === '"') {
      // Copy the string through its closing quote, stepping over each escaped character.
      let end = i + 1;
      while (line[end] !== '"') end += line[end] === "\\" ? 2 : 1;
      out += line.slice(i, end + 1);
      i = end;
    } else if (ch === "{" || ch === "[") {
      // An empty object or array stays on one line.
      let next = i + 1;
      while (/\s/.test(line[next] ?? "")) next++;
      if (line[next] === "}" || line[next] === "]") {
        out += ch + line[next];
        i = next;
      } else {
        depth++;
        out += `${ch}\n${INDENT.repeat(depth)}`;
      }
    } else if (ch === "}" || ch === "]") {
      depth--;
      out += `\n${INDENT.repeat(depth)}${ch}`;
    } else if (ch === ",") {
      out += `,\n${INDENT.repeat(depth)}`;
    } else if (ch === ":") {
      out += ": ";
    } else if (!/\s/.test(ch)) {
      out += ch;
    }
  }
  return out;
}

/**
 * Whether a line holds the filter's text, compared without regard to case (FLOW-READ-RAW-LOG-R12). The text is taken
 * literally. A case-blind expression finds it without making a lowered copy of every line the pane holds.
 */
export function lineMatcher(text: string): (line: string) => boolean {
  const pattern = new RegExp(text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "i");
  return (line) => pattern.test(line);
}

/** The bytes a line takes as UTF-8, as Core counts a line's size (PROTO-CORE-API-R138). */
export function lineBytes(line: string): number {
  let bytes = 0;
  for (let i = 0; i < line.length; i++) {
    const unit = line.charCodeAt(i);
    if (unit < 0x80) bytes += 1;
    else if (unit < 0x800) bytes += 2;
    else if (unit >= 0xd800 && unit <= 0xdbff) {
      // A pair of surrogates is one character of four bytes.
      bytes += 4;
      i++;
    } else bytes += 3;
  }
  return bytes;
}

/** The text a line holds: the whole line, or the start Core sent of a line it cut (FLOW-READ-RAW-LOG-R11). */
export function lineText(entry: RawLogEntry): string {
  return "line" in entry ? entry.line : entry.head;
}

/** `ts` as 24-hour `HH:MM:SS.mmm` in the user's local time (FLOW-READ-RAW-LOG-R03); as sent when it isn't a time. */
export function rawLogTime(ts: string): string {
  const at = new Date(ts);
  if (Number.isNaN(at.getTime())) return ts;
  const two = (n: number) => String(n).padStart(2, "0");
  return `${two(at.getHours())}:${two(at.getMinutes())}:${two(at.getSeconds())}.${String(at.getMilliseconds()).padStart(3, "0")}`;
}
