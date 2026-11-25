// @vitest-environment jsdom
// The archived note's Resume while Core answers (issue #483; docs/design/DESIGN.md, Motion): the ring for work of
// unknown length leads "Resuming…", and both labels hold one cell, so the button keeps its width as its label changes.

import { cleanup, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { Sending } from "../session/follow";
import { ArchivedNote } from "./ArchivedNote";

afterEach(cleanup);

const draw = (resuming?: Sending) => render(createElement(ArchivedNote, { onResume: () => {}, resuming }));
/** Each label the button holds, with whether it is the one shown and whether a ring leads it. */
const labels = (button: HTMLElement) =>
  [...button.querySelectorAll(".button-label")].map((label) => ({
    text: label.textContent,
    shown: label.hasAttribute("data-shown"),
    ring: label.querySelector(".button-ring") !== null,
  }));

describe("Resume on the archived note", () => {
  it("holds both its labels in one cell at rest, showing Resume with no ring", () => {
    draw();
    const resume = screen.getByRole("button", { name: "Resume" });
    expect(labels(resume)).toEqual([
      { text: "Resume", shown: true, ring: false },
      { text: "Resuming…", shown: false, ring: true },
    ]);
  });

  it("shows Resuming… led by the turning ring while Core answers, unavailable yet focusable", () => {
    draw({ state: "sending" });
    const resume = screen.getByRole("button", { name: "Resuming…" });
    expect(resume.getAttribute("aria-disabled")).toBe("true");
    expect(labels(resume)).toEqual([
      { text: "Resume", shown: false, ring: false },
      { text: "Resuming…", shown: true, ring: true },
    ]);
  });

  it("looks the same for the whole wait, the folder being put back included", () => {
    draw({ state: "sending", progress: { phase: "puttingBack" } });
    const resume = screen.getByRole("button", { name: "Resuming…" });
    expect(labels(resume).find((l) => l.shown)).toEqual({ text: "Resuming…", shown: true, ring: true });
  });
});
