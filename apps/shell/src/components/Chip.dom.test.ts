// @vitest-environment jsdom
// A chip in a DOM (docs/design/COMPOSER.md, The session controls): a running session's chip while a change is on its way.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { Chip } from "./Chip";

afterEach(cleanup);

const options = [
  { id: "opus", label: "Opus" },
  { id: "sonnet", label: "Sonnet" },
];
type Props = Partial<Parameters<typeof Chip>[0]>;
const draw = (props: Props = {}) => createElement(Chip, { name: "Model", value: "opus", options, onChoose: () => {}, ...props });
const chip = () => screen.getByRole("button", { name: /^Model:/ });

describe("a chip whose change is on its way", () => {
  // verifies: FLOW-NEW-SESSION-R30
  it("keeps keyboard focus and its old value, and does not open until Core answers", () => {
    const chosen: string[] = [];
    const { rerender } = render(draw({ onChoose: (id) => chosen.push(id) }));
    act(() => chip().focus());
    fireEvent.keyDown(chip(), { key: "ArrowDown" });
    fireEvent.click(screen.getByRole("option", { name: /Sonnet/ }));
    expect(chosen).toEqual(["sonnet"]);
    // The window keeps the old value until Core answers.
    rerender(draw({ waiting: true }));
    expect(document.activeElement).toBe(chip());
    expect(chip().getAttribute("aria-label")).toBe("Model: Opus");
    expect((chip() as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(chip());
    fireEvent.keyDown(chip(), { key: "ArrowDown" });
    expect(screen.queryByRole("listbox")).toBeNull();
    // Core accepted: the new value, focus still on the chip.
    rerender(draw({ value: "sonnet" }));
    expect(chip().getAttribute("aria-label")).toBe("Model: Sonnet");
    expect(document.activeElement).toBe(chip());
  });

  it("tells the window each time it opens", () => {
    let opened = 0;
    render(draw({ onOpen: () => opened++ }));
    fireEvent.click(chip());
    expect(opened).toBe(1);
    // Closing is not opening.
    fireEvent.click(chip());
    expect(opened).toBe(1);
  });
});
