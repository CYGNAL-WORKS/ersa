import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ContextRing } from "./ContextRing";

// LAY-SHELL, Context: near full from 85 percent, on a tile and in the session controls. A layout brief, so no IDs.
describe("the context ring", () => {
  const draw = (used: number | undefined, size?: "ring" | "ring-sm") => renderToStaticMarkup(createElement(ContextRing, { used, size }));

  it("is near full from 85 percent at the session controls' size, with its readout", () => {
    expect(draw(0.85)).toContain('data-size="ring" data-near-full="true"');
    expect(draw(0.85)).toContain(">85%</span>");
    expect(draw(0.84)).not.toContain("data-near-full");
  });

  it("draws a tile's ring smaller, with no readout", () => {
    const html = draw(0.42, "ring-sm");
    expect(html).toContain('viewBox="0 0 12 12"');
    expect(html).not.toContain("context-ring-readout");
    expect(html).toContain('aria-label="Context: 42 percent used"');
  });
});
