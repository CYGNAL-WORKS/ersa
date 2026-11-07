// @vitest-environment jsdom
// The menu's keyboard model in a DOM (docs/design/RAIL.md, Menus): where focus goes as it opens, how the arrow keys
// move through the items it can take, how Right and Left open and close a submenu, and what Escape and choosing close.
// RAIL.md is a design document with no requirement IDs.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement, useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { Menu, type MenuClose, type MenuEntry } from "./Menu";

const chosen: string[] = [];
const closed: MenuClose[] = [];

const entries: MenuEntry[] = [
  { kind: "item", label: "Pin", onChoose: () => chosen.push("Pin") },
  { kind: "item", label: "Fork", unavailable: "Not now" },
  { kind: "rule" },
  {
    kind: "submenu",
    label: "Move to section",
    entries: [
      { kind: "item", label: "Ideas", checked: true, onChoose: () => chosen.push("Ideas") },
      { kind: "item", label: "No section", onChoose: () => chosen.push("No section") },
    ],
  },
  { kind: "item", label: "Delete…", danger: true, onChoose: () => chosen.push("Delete") },
];

/** A menu as its owner holds it: open until it closes, with a button standing in for what opened it. */
function Owner({ focus }: { focus: "item" | "menu" }) {
  const [open, setOpen] = useState(true);
  return createElement(
    "div",
    null,
    createElement("button", { type: "button" }, "Opener"),
    createElement(Menu, {
      label: "Actions",
      entries,
      open,
      place: { anchor: "--a" },
      focus,
      onClose: (how: MenuClose) => {
        closed.push(how);
        setOpen(false);
      },
    }),
  );
}

const key = (k: string) => fireEvent.keyDown(document.activeElement as Element, { key: k });
const focused = () => document.activeElement?.textContent;

afterEach(() => {
  cleanup();
  chosen.length = 0;
  closed.length = 0;
});

describe("a menu from the keyboard", () => {
  it("opens with focus on the first item it can take, and the arrows skip what it can't, wrapping round", () => {
    render(createElement(Owner, { focus: "item" }));
    expect(focused()).toBe("Pin");
    key("ArrowDown");
    // Fork is unavailable and the rule is not an item, so the next item is the submenu.
    expect(focused()).toBe("Move to section");
    key("ArrowDown");
    expect(focused()).toBe("Delete…");
    key("ArrowDown");
    expect(focused()).toBe("Pin");
    key("ArrowUp");
    expect(focused()).toBe("Delete…");
    key("Home");
    expect(focused()).toBe("Pin");
    key("End");
    expect(focused()).toBe("Delete…");
  });

  it("shows an unavailable item with its reason, and choosing it does nothing", () => {
    render(createElement(Owner, { focus: "item" }));
    const fork = screen.getByRole("menuitem", { name: /^Fork/ });
    expect(fork.getAttribute("aria-disabled")).toBe("true");
    expect(fork.textContent).toContain("Not now");
    fireEvent.click(fork);
    expect(closed).toEqual([]);
  });

  it("opens a submenu with Right, on its first item, and Left closes it back to its item", () => {
    render(createElement(Owner, { focus: "item" }));
    key("ArrowDown");
    key("ArrowRight");
    expect(focused()).toBe("Ideas");
    expect(screen.getByRole("menuitemradio", { name: "Ideas" }).getAttribute("aria-checked")).toBe("true");
    key("ArrowLeft");
    // It fades out, then goes: its item says it is closed at once.
    expect(screen.getByRole("menuitem", { name: "Move to section" }).getAttribute("aria-expanded")).toBe("false");
    expect(focused()).toBe("Move to section");
    expect(closed).toEqual([]);
  });

  it("closes only the innermost menu with Escape, then the menu itself", () => {
    render(createElement(Owner, { focus: "item" }));
    key("ArrowDown");
    key("ArrowRight");
    key("Escape");
    // It fades out, then goes: its item says it is closed at once.
    expect(screen.getByRole("menuitem", { name: "Move to section" }).getAttribute("aria-expanded")).toBe("false");
    expect(focused()).toBe("Move to section");
    expect(closed).toEqual([]);
    key("Escape");
    expect(closed).toEqual(["escape"]);
  });

  it("closes every menu when an item in a submenu is chosen, then does what it says", () => {
    render(createElement(Owner, { focus: "item" }));
    key("ArrowDown");
    key("ArrowRight");
    key("ArrowDown");
    fireEvent.click(document.activeElement as Element);
    expect(closed).toEqual(["choose"]);
    expect(chosen).toEqual(["No section"]);
  });
});

describe("a menu from the pointer", () => {
  it("takes focus itself, and the arrows move from it to the items", () => {
    render(createElement(Owner, { focus: "menu" }));
    expect(document.activeElement?.getAttribute("role")).toBe("menu");
    key("ArrowDown");
    expect(focused()).toBe("Pin");
  });

  it("closes when focus leaves it for somewhere else, and gives no focus back", () => {
    render(createElement(Owner, { focus: "item" }));
    fireEvent.blur(document.activeElement as Element, { relatedTarget: screen.getByRole("button", { name: "Opener" }) });
    expect(closed).toEqual(["away"]);
  });

  it("closes on a press outside it", () => {
    render(createElement(Owner, { focus: "menu" }));
    fireEvent.pointerDown(document.body);
    expect(closed).toEqual(["away"]);
  });
});
