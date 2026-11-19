// @vitest-environment jsdom
// The icon button that opens a rail menu (docs/design/RAIL.md, Menus): the arrange button and a heading's More.
// RAIL.md and LAY-SHELL carry no requirement IDs for these.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Ellipsis } from "lucide-react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { MenuButton } from "./MenuButton";

afterEach(cleanup);

const button = (unavailable: boolean) =>
  createElement(MenuButton, { icon: Ellipsis, label: "Arrange sessions", unavailable, entries: [{ kind: "item", label: "Reset to defaults", onChoose: () => {} }] });

describe("a menu button", () => {
  it("opens from the keyboard with focus on the first item, and Escape gives focus back to it", () => {
    render(button(false));
    const opener = screen.getByRole("button", { name: "Arrange sessions" });
    opener.focus();
    fireEvent.click(opener, { detail: 0 });
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Reset to defaults" }));
    expect(opener.getAttribute("aria-expanded")).toBe("true");
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(opener.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(opener);
  });

  it("closes its menu when it becomes unavailable, as when Ersa loses Core, and keeps the keyboard's place on itself", async () => {
    const { rerender } = render(button(false));
    const opener = screen.getByRole("button", { name: "Arrange sessions" });
    opener.focus();
    fireEvent.click(opener, { detail: 0 });
    expect(document.activeElement?.getAttribute("role")).toBe("menuitem");
    await act(async () => rerender(button(true)));
    expect(opener.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(opener);
    // Unavailable, it keeps focus but opens nothing.
    expect(opener.getAttribute("aria-disabled")).toBe("true");
    expect((opener as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(opener, { detail: 0 });
    expect(opener.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByRole("menuitem")).toBeNull();
  });
});
