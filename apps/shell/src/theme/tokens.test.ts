// Holds the design rule "every colour, size, radius and duration comes from a
// token in DESIGN.md" (CLAUDE.md, Design) as a check rather than a review.

import { readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";
// @ts-expect-error: a plain script without type declarations.
import { css, tokensFrom } from "../../scripts/gen-tokens.mjs";

const SRC = join(import.meta.dirname, "..");

function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return /\.(css|tsx?)$/.test(entry.name) && !/\.(gen|test)\./.test(entry.name) ? [path] : [];
  });
}

// Raw values a token should supply. A hairline border is the one exception:
// DESIGN.md has no token for line width (an inbox entry proposes one), so
// `1px` passes in a `border*` declaration and nowhere else.
const RAW = [
  { what: "a hex colour", pattern: /#[0-9a-f]{3,8}\b/gi },
  { what: "a colour function", pattern: /\b(rgba?|hsla?|oklch|lab|lch|color-mix)\(/gi },
  { what: "an easing", pattern: /\bcubic-bezier\(/gi },
  {
    what: "a length, duration or percentage",
    pattern: /(?<![\w.-])\d*\.?\d+(px|rem|em|vh|vw|dvh|svh|pt|ms|s|%)(?![\w-])/gi,
  },
  { what: "a font weight", pattern: /font-weight\s*:(?!\s*var\()\s*[^;]+/gi },
];

// The file that declares the bundled fonts: its weights and ranges describe
// the font files, not the interface.
const FONT_FILES = join("theme", "fonts.css");

/** The CSS property a line declares, if it declares one. */
const propertyOf = (line: string) => line.match(/^\s*([a-z-]+)\s*:/)?.[1] ?? "";

const allowed = (match: string, line: string) => match === "1px" && propertyOf(line).startsWith("border");

const flagged = (line: string) =>
  RAW.flatMap(({ pattern }) => [...line.matchAll(pattern)].map((m) => m[0])).filter((m) => !allowed(m, line));

describe("interface source", () => {
  it("uses tokens for every colour, size, radius, weight and duration", () => {
    const found: string[] = [];
    for (const file of sourceFiles(SRC)) {
      if (relative(SRC, file) === FONT_FILES) continue;
      readFileSync(file, "utf8")
        .split("\n")
        .forEach((line, i) => {
          for (const match of flagged(line)) found.push(`${relative(SRC, file)}:${i + 1}: ${match}`);
        });
    }
    expect(found).toEqual([]);
  });
});

describe("the raw-value check", () => {
  it("allows a hairline only on borders", () => {
    expect(flagged("  border: 1px solid var(--color-line);")).toEqual([]);
    expect(flagged("  border-right: 1px solid var(--color-line);")).toEqual([]);
    expect(flagged("  outline-offset: 1px;")).toEqual(["1px"]);
    expect(flagged("  box-shadow: 0 0 0 1px var(--color-line);")).toEqual(["1px"]);
  });

  it("catches colours, percentages and font-weight keywords", () => {
    expect(flagged("  color: #fff;")).toEqual(["#fff"]);
    expect(flagged("  width: 22%;")).toEqual(["22%"]);
    expect(flagged("  font-weight: normal;")).toEqual(["font-weight: normal"]);
    expect(flagged("  font-weight: var(--type-heading-weight);")).toEqual([]);
    expect(flagged("  transition: opacity 160ms;")).toEqual(["160ms"]);
  });
});

describe("generated tokens", () => {
  const tokens = tokensFrom(readFileSync(join(SRC, "..", "..", "..", "docs", "design", "DESIGN.md"), "utf8"));
  const generated = css(tokens);

  it("give light and dark the same colour names", () => {
    expect(Object.keys(tokens.colors.dark)).toEqual(Object.keys(tokens.colors.light));
  });

  it("carry every DESIGN.md colour in both themes", () => {
    for (const [name, value] of Object.entries(tokens.colors.light)) {
      expect(generated).toContain(`--color-${name}: ${value};`);
    }
    for (const [name, value] of Object.entries(tokens.colors.dark)) {
      expect(generated).toContain(`--color-${name}: ${value};`);
    }
  });

  it("resolve references between tokens", () => {
    expect(generated).not.toMatch(/\{[\w.]+\}/);
    expect(generated).toContain(`--type-ui-small-family: ${tokens.typography.ui.fontFamily};`);
  });
});
