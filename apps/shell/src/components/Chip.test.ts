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

describe("a chip as a pill (docs/design/COMPOSER.md, The session controls)", () => {
  const options = [{ id: "a", label: "Alpha" }];
  const html = (props: Partial<Parameters<typeof Chip>[0]> = {}) =>
    renderToStaticMarkup(createElement(Chip, { name: "Workspace", value: "a", options, onChoose: () => {}, ...props }));

  it("ends with a chevron when it opens a list", () => {
    expect(html()).toContain("chip-chevron");
  });

  it("under a running session shows its value without the chevron, and does not open", () => {
    const shown = html({ readOnly: true });
    expect(shown).not.toContain("chip-chevron");
    expect(shown).toMatch(/<button[^>]*disabled/);
    expect(shown).toContain("data-read-only");
    expect(shown).toContain("Alpha");
  });

  it("while detecting, leads with the context ring's track and a turning arc, and has no chevron", () => {
    const shown = html({ value: undefined, placeholder: "Detecting backends…", pending: true, options: [], disabled: true });
    expect(shown).toContain("data-turning");
    expect(shown).not.toContain("chip-chevron");
  });

  it("reads a placeholder that invites the user, such as Choose a folder, in ink", () => {
    expect(html({ value: undefined, placeholder: "Choose a folder", invites: true })).toContain("data-invites");
    expect(html({ value: undefined, placeholder: "No backend" })).not.toContain("data-invites");
    // Unavailable, as while the first folder is added, it reads ink-faint like any chip.
    expect(html({ value: undefined, placeholder: "Choose a folder", invites: true, disabled: true })).not.toContain("data-invites");
  });
});
