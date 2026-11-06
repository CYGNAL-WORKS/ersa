// A chip rendered open to markup, as the window would first draw it.

import { Plus } from "lucide-react";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { type ChipAction, Chip } from "./Chip";

const open = (action: ChipAction) =>
  renderToStaticMarkup(createElement(Chip, { name: "Project", value: "p1", options: [{ id: "p1", label: "ersa" }], onChoose: () => {}, action, initialOpen: true }));

describe("the action that ends a chip's list", () => {
  it("shows its shortcut only while it can be taken, so no shortcut is offered that does nothing", () => {
    const action = { label: "Add project…", icon: Plus, shortcut: "Ctrl+O" };
    const unavailable = open(action);
    expect(unavailable).toContain('aria-disabled="true"');
    expect(unavailable).not.toContain("Ctrl+O");
    const available = open({ ...action, onChoose: () => {} });
    expect(available).not.toContain("aria-disabled");
    expect(available).toContain("Ctrl+O");
  });
});
