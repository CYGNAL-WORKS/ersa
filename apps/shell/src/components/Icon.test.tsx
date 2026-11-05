import { Plus } from "lucide-react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Icon, IconButton } from "./Icon";

describe("Icon", () => {
  it("draws a Lucide icon with a 2 unit stroke, hidden from assistive technology", () => {
    const html = renderToStaticMarkup(<Icon icon={Plus} />);
    expect(html).toContain('stroke-width="2"');
    expect(html).toContain('stroke-linecap="round"');
    expect(html).toContain('aria-hidden="true"');
    expect(html).toContain('data-size="icon"');
  });
});

describe("IconButton", () => {
  it("gives an icon with no visible label an accessible name and a tooltip", () => {
    const html = renderToStaticMarkup(<IconButton icon={Plus} label="New session" />);
    expect(html).toContain('aria-label="New session"');
    expect(html).toContain('title="New session"');
  });
});
