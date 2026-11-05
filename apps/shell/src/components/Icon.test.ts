import { Plus } from "lucide-react";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Icon, IconButton } from "./Icon";

describe("Icon", () => {
  it("draws Lucide's own stroke with round ends, hidden from assistive technology", () => {
    const html = renderToStaticMarkup(createElement(Icon, { icon: Plus }));
    // Lucide's defaults, which DESIGN.md (Iconography) takes as they are: a change in the package shows here.
    expect(html).toContain('stroke-width="2"');
    expect(html).toContain('stroke-linecap="round"');
    expect(html).toContain('viewBox="0 0 24 24"');
    expect(html).toContain('aria-hidden="true"');
  });

  it("takes its size from a token by name, the ui size unless told", () => {
    expect(renderToStaticMarkup(createElement(Icon, { icon: Plus }))).toContain('data-size="icon"');
    expect(renderToStaticMarkup(createElement(Icon, { icon: Plus, size: "icon-sm" }))).toContain('data-size="icon-sm"');
  });
});

describe("IconButton", () => {
  it("gives an icon with no visible label an accessible name and a tooltip", () => {
    const html = renderToStaticMarkup(createElement(IconButton, { icon: Plus, label: "New session" }));
    expect(html).toContain('aria-label="New session"');
    expect(html).toContain('title="New session"');
    expect(html).toContain('type="button"');
  });

  it("passes its icon's size on", () => {
    expect(renderToStaticMarkup(createElement(IconButton, { icon: Plus, label: "New session", size: "icon-sm" }))).toContain('data-size="icon-sm"');
  });
});
