// @vitest-environment jsdom
// The menu's keyboard model in a DOM (docs/design/RAIL.md, Menus): where focus goes as it opens, how the arrow keys
// move through the items it can take, how Right and Left open and close a submenu, and what Escape and choosing close.
// RAIL.md is a design document with no requirement IDs.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
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
      { kind: "item", label: "No section", checked: false, onChoose: () => chosen.push("No section") },
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

  // #374: moving from a submenu's item to a plain one, as from Group by to Reset to defaults, leaves no submenu open.
  it("opens a submenu under the pointer, keeps it open inside it, and closes it on a plain item or one it can't take", () => {
    render(createElement(Owner, { focus: "menu" }));
    const sub = screen.getByRole("menuitem", { name: "Move to section" });
    const expanded = () => sub.getAttribute("aria-expanded");
    fireEvent.pointerEnter(sub);
    expect(expanded()).toBe("true");
    // Into the submenu: it stays open.
    fireEvent.pointerEnter(screen.getByRole("menuitemradio", { name: "Ideas" }));
    expect(expanded()).toBe("true");
    fireEvent.pointerEnter(screen.getByRole("menuitem", { name: "Delete…" }));
    expect(expanded()).toBe("false");
    fireEvent.pointerEnter(sub);
    expect(expanded()).toBe("true");
    // An item it can't take closes it too.
    fireEvent.pointerEnter(screen.getByRole("menuitem", { name: /Fork/ }));
    expect(expanded()).toBe("false");
    expect(closed).toEqual([]);
  });

  it("gives keyboard focus in a submenu the pointer closes to the item under the pointer, so the menu keeps its keys", () => {
    render(createElement(Owner, { focus: "item" }));
    key("ArrowDown");
    key("ArrowRight");
    expect(focused()).toBe("Ideas");
    fireEvent.pointerEnter(screen.getByRole("menuitem", { name: "Delete…" }));
    expect(screen.getByRole("menuitem", { name: "Move to section" }).getAttribute("aria-expanded")).toBe("false");
    expect(focused()).toBe("Delete…");
    key("ArrowUp");
    expect(focused()).toBe("Move to section");
    key("Escape");
    expect(closed).toEqual(["escape"]);
  });

  it("closes on a press outside it", () => {
    render(createElement(Owner, { focus: "menu" }));
    fireEvent.pointerDown(document.body);
    expect(closed).toEqual(["away"]);
  });
});

describe("a menu opened at the pointer", () => {
  // The window is 800 by 600; the menu, as laid out, 200 by 300. (jsdom lays nothing out, so its size is given.)
  const W = 800;
  const H = 600;
  const size = { width: 200, height: 300 };
  const realRect = HTMLElement.prototype.getBoundingClientRect;

  afterEach(() => {
    HTMLElement.prototype.getBoundingClientRect = realRect;
    size.height = 300;
    size.width = 200;
  });

  /** Opens a menu at the press and gives back its box, as the window shows it. */
  function openAt(x: number, y: number) {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: W });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: H });
    HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
      if (!this.classList.contains("menu")) return realRect.call(this);
      const left = parseFloat(this.style.left);
      const top = parseFloat(this.style.top);
      return { left, top, right: left + size.width, bottom: top + size.height, width: size.width, height: size.height, x: left, y: top, toJSON: () => ({}) };
    };
    render(createElement(Menu, { label: "Actions", entries, open: true, place: { x, y }, focus: "menu", onClose: () => {} }));
    return screen.getByRole("menu").getBoundingClientRect();
  }

  // verifies: FLOW-ORGANIZE-SESSIONS-R28
  // verifies: FLOW-ORGANIZE-SESSIONS-R29
  // Near each edge and corner, every item stays inside the window: the menu opens the other way, and stays inside.
  it.each([
    ["the foot", 100, 590, 300, { left: 100, top: 290 }],
    ["the right edge", 790, 100, 300, { left: 590, top: 100 }],
    ["the bottom right corner", 795, 595, 300, { left: 595, top: 295 }],
    ["the top left corner", 2, 2, 300, { left: 2, top: 2 }],
    ["a press where the menu fits neither below nor above", 150, 200, 500, { left: 150, top: 0 }],
  ])("near %s, the whole menu is inside the window", (_, x, y, height, want) => {
    size.height = height;
    const box = openAt(x, y);
    expect(box.left).toBe(want.left);
    expect(box.top).toBe(want.top);
    expect(box.left).toBeGreaterThanOrEqual(0);
    expect(box.top).toBeGreaterThanOrEqual(0);
    expect(box.right).toBeLessThanOrEqual(W);
    expect(box.bottom).toBeLessThanOrEqual(H);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R29
  it("larger than the window both ways, it sits against the window's left and top edges", () => {
    size.width = 900;
    size.height = 700;
    const box = openAt(400, 300);
    expect(box.left).toBe(0);
    expect(box.top).toBe(0);
  });

  // verifies: FLOW-ORGANIZE-SESSIONS-R30
  it("is placed again when it grows while open, or the window shrinks", () => {
    // The fake observer reports a change only for the elements it was asked to watch.
    let callback: () => void = () => {};
    const watched = new Set<Element>();
    const grew = (el: Element) => watched.has(el) && callback();
    const real = globalThis.ResizeObserver;
    globalThis.ResizeObserver = class {
      constructor(cb: () => void) {
        callback = cb;
      }
      observe(el: Element) {
        watched.add(el);
      }
      disconnect() {}
    } as unknown as typeof ResizeObserver;
    try {
      openAt(100, 300);
      const menu = screen.getByRole("menu");
      expect(menu.getBoundingClientRect().top).toBe(300);
      // An item gains its reason: the menu grows past the foot, and moves up to stay inside.
      size.height = 350;
      act(() => {
        grew(menu);
      });
      expect(menu.getBoundingClientRect().bottom).toBeLessThanOrEqual(H);
      expect(menu.getBoundingClientRect().top).toBe(0);
      // The window shrinks: it moves in again.
      Object.defineProperty(window, "innerWidth", { configurable: true, value: 250 });
      act(() => {
        window.dispatchEvent(new Event("resize"));
      });
      expect(menu.getBoundingClientRect().right).toBeLessThanOrEqual(250);
    } finally {
      globalThis.ResizeObserver = real;
    }
  });
});
