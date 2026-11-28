// The Clean up list's own CSS (docs/design/SETTINGS.md, Clean up), for what a window test can't see in jsdom.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const css = readFileSync(join(import.meta.dirname, "CleanUp.css"), "utf8").replace(/\r\n/g, "\n");

/** The declarations of the rule whose selector is exactly `selector`. */
function rule(selector: string): string {
  const head = `\n${selector} {\n`;
  const start = css.indexOf(head);
  if (start < 0) return "";
  const from = start + head.length;
  return css.slice(from, css.indexOf("\n}", from));
}

describe("the list", () => {
  it("shows keyboard focus after a clean-up with a token edge, not the browser's ring", () => {
    expect(rule(".clean-up-body:focus")).toContain("outline: none;");
    expect(rule(".clean-up-body:focus-visible")).toContain("box-shadow: inset 0 0 0 var(--line-hairline) var(--color-ink);");
  });

  it("clips what it holds only while it grows, so a row's edge reaches past its sides at rest", () => {
    expect(rule(".clean-up-body")).not.toContain("overflow");
    expect(rule(".clean-up-body[data-easing]")).toContain("overflow: clip;");
  });
});
