import { describe, expect, it } from "vitest";
import { indentJson, lineBytes, lineMatcher } from "./rawLogText";

describe("a line indented as JSON", () => {
  // verifies: FLOW-READ-RAW-LOG-R10
  it("lays a whole line of JSON out with indentation, two spaces a level", () => {
    expect(indentJson('{"type":"user","message":{"content":["a",1]},"empty":{},"none":[]}')).toBe(
      ['{', '  "type": "user",', '  "message": {', '    "content": [', '      "a",', "      1", "    ]", "  },", '  "empty": {},', '  "none": []', "}"].join("\n"),
    );
  });

  // verifies: FLOW-READ-RAW-LOG-R10
  it("keeps every token as it crossed: numbers, escapes and strings holding brackets", () => {
    // A parse and print would write 1.0 as 1, 1e3 as 1000, round the big integer and turn the escape into é.
    const line = '{"a":1.0,"b":1e3,"c":12345678901234567890,"d":"caf\\u00e9","e":"{[,:]} \\"q\\""}';
    const shown = indentJson(line);
    expect(shown).toContain('"a": 1.0,');
    expect(shown).toContain('"b": 1e3,');
    expect(shown).toContain('"c": 12345678901234567890,');
    expect(shown).toContain('"d": "caf\\u00e9",');
    expect(shown).toContain('"e": "{[,:]} \\"q\\""');
    // Only whitespace between tokens changes.
    expect(shown?.replace(/\s/g, "")).toBe(line.replace(/\s/g, ""));
  });

  // verifies: FLOW-READ-RAW-LOG-R10
  it("leaves a line that is not JSON, or JSON with nothing to indent, as it is", () => {
    expect(indentJson("Claude Code 2.1.316 started")).toBeUndefined();
    expect(indentJson('{"type":"user"')).toBeUndefined();
    expect(indentJson('"just a string"')).toBeUndefined();
    expect(indentJson("42")).toBeUndefined();
  });
});

describe("the filter", () => {
  // verifies: FLOW-READ-RAW-LOG-R12
  it("finds the text anywhere in a line, whatever its case, and takes it literally", () => {
    const matches = lineMatcher("ERROR (x)");
    expect(matches("an error (X) here")).toBe(true);
    expect(matches("an error x here")).toBe(false);
    expect(lineMatcher(".*")("abc")).toBe(false);
  });
});

describe("a line's size", () => {
  it("counts the bytes the line takes as UTF-8", () => {
    expect(lineBytes("abc")).toBe(3);
    expect(lineBytes("é")).toBe(2);
    expect(lineBytes("😀")).toBe(4);
  });
});
