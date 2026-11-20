// @vitest-environment jsdom
// Which side a chip's list and the attach menu opened on (docs/design/COMPOSER.md, A chip's list and The attach
// menu; #374): each says it on the layer, so its entrance slides away from what opened it even when the window
// flipped it. COMPOSER.md is a design document with no requirement IDs. The slide itself is the gallery's check
// (e2e/gallery.spec.ts), since jsdom lays nothing out and runs no animations.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { AttachMenu } from "./AttachMenu";
import { Chip } from "./Chip";

const realRect = HTMLElement.prototype.getBoundingClientRect;

/** Lays out the opener at 500 to 528 down the window, and the layer, once open, at `layerTop`, 100 high. */
function layOut(layerTop: number) {
  HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
    const at = (top: number, height: number) => ({ left: 0, top, right: 200, bottom: top + height, width: 200, height, x: 0, y: top, toJSON: () => ({}) });
    if (this.classList.contains("chip-list")) return at(layerTop, 100);
    if (this.classList.contains("chip") || this.classList.contains("composer-attach")) return at(500, 28);
    return realRect.call(this);
  };
}

afterEach(() => {
  cleanup();
  HTMLElement.prototype.getBoundingClientRect = realRect;
});

const chip = () => createElement(Chip, { name: "Model", value: "opus", options: [{ id: "opus", label: "Opus" }], onChoose: () => {} });
const attach = () => createElement(AttachMenu, { onChoose: () => {} });

describe("the side a layer opened on", () => {
  it.each([
    ["a chip's list beneath its chip", chip, "button", "listbox", 532, "below"],
    ["a chip's list the window flipped above its chip", chip, "button", "listbox", 396, "above"],
    ["the attach menu above attach", attach, "button", "menu", 396, "above"],
    ["the attach menu the window flipped below attach", attach, "button", "menu", 532, "below"],
  ] as const)("is said on %s", (_, draw, opener, layer, top, side) => {
    layOut(top);
    render(draw());
    fireEvent.click(screen.getByRole(opener));
    expect(screen.getByRole(layer).dataset.side).toBe(side);
  });

  it("is measured again each time the layer opens", () => {
    layOut(532);
    render(chip());
    fireEvent.click(screen.getByRole("button"));
    expect(screen.getByRole("listbox").dataset.side).toBe("below");
    fireEvent.click(screen.getByRole("button"));
    layOut(396);
    fireEvent.click(screen.getByRole("button"));
    expect(screen.getByRole("listbox").dataset.side).toBe("above");
  });
});
