// @vitest-environment jsdom
// The developer pane's toolbar over its list (FLOW-READ-RAW-LOG, step 5; docs/design/WORKBENCH.md, The developer
// pane): the filter field lists only the lines that contain its text, whatever its case, says how many of the lines
// held those are, and goes on following the end. jsdom does no layout, so the list's height is stubbed and the row
// height is the design's token, set on the page as the stylesheet would.

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RawLogEntry } from "../protocol/core-api.gen";
import type { RawLogView } from "../session/rawLog";
import { DeveloperPane } from "./DeveloperPane";

const ROW = 24;
const HEIGHT = 240;

beforeEach(() => {
  document.documentElement.style.setProperty("--size-log-row", `${ROW}px`);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(function (this: HTMLElement) {
    return this.classList.contains("raw-log-list") ? HEIGHT : 0;
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

/** Line n reads "ok n", and every fifth "Error n" instead. */
const entry = (index: number): RawLogEntry => ({ index, ts: "2026-11-22T10:00:00.000Z", dir: "backend", line: index % 5 === 0 ? `Error ${index}` : `ok ${index}` });
const entries = (from: number, to: number) => Array.from({ length: to - from }, (_, i) => entry(from + i));
const viewOf = (lines: RawLogEntry[], more: Partial<RawLogView> = {}): RawLogView => ({ session: "s1", lines, read: true, earlier: false, ...more });

function show(view: RawLogView | undefined, session = "s1") {
  const shown = render(createElement(DeveloperPane, { session, view, backend: "Claude Code" }));
  return {
    update: (next: RawLogView | undefined, nextSession = session) =>
      shown.rerender(createElement(DeveloperPane, { session: nextSession, view: next, backend: "Claude Code" })),
  };
}

const field = () => screen.getByRole("searchbox", { name: "Filter lines" });
const type = (text: string) => fireEvent.change(field(), { target: { value: text } });
const list = () => screen.getByRole("listbox", { name: "Raw log" });
const texts = () => within(list()).queryAllByRole("option").map((row) => row.querySelector(".raw-log-text")?.textContent);
const count = () => document.querySelector(".developer-count");

describe("the filter", () => {
  // verifies: FLOW-READ-RAW-LOG-R12
  it("lists only the lines that contain its text, whatever the case, and says how many of the lines held they are", () => {
    show(viewOf(entries(0, 12)));
    expect(field().getAttribute("placeholder")).toBe("Filter lines");
    expect(count()?.hasAttribute("data-shown")).toBe(false);
    type("eRRoR");
    expect(texts()).toEqual(["Error 0", "Error 5", "Error 10"]);
    expect(count()?.hasAttribute("data-shown")).toBe(true);
    expect(count()?.textContent).toBe("3 of 12");
    // The count is read with the field.
    expect(field().getAttribute("aria-describedby")).toBe(count()?.id);
  });

  // verifies: FLOW-READ-RAW-LOG-R12
  it("lists every line again once the field is emptied, by hand or by Escape", () => {
    show(viewOf(entries(0, 4)));
    type("error");
    expect(texts()).toHaveLength(1);
    type("");
    expect(texts()).toHaveLength(4);
    expect(count()?.hasAttribute("data-shown")).toBe(false);
    type("error");
    fireEvent.keyDown(field(), { key: "Escape" });
    expect((field() as HTMLInputElement).value).toBe("");
    expect(texts()).toHaveLength(4);
  });

  // verifies: FLOW-READ-RAW-LOG-R12
  it("goes on following the end as lines that contain the text arrive, and counts them", () => {
    const { update } = show(viewOf(entries(0, 100)));
    type("error");
    // Twenty matching lines of 24 pixels in a list 240 high end at 240.
    expect(list().scrollTop).toBe(20 * ROW - HEIGHT);
    update(viewOf(entries(0, 110)));
    expect(texts().at(-1)).toBe("Error 105");
    expect(list().scrollTop).toBe(22 * ROW - HEIGHT);
    expect(count()?.textContent).toBe("22 of 110");
  });

  // verifies: FLOW-READ-RAW-LOG-R12
  it("writes the count with a separator for thousands", () => {
    show(viewOf(entries(0, 12_000)));
    type("ok");
    expect(count()?.textContent).toBe("9,600 of 12,000");
  });

  // verifies: FLOW-READ-RAW-LOG-R14
  it("says so when no line contains the text", () => {
    show(viewOf(entries(0, 4)));
    type("nowhere");
    expect(screen.getByText("No lines contain that text.")).toBeTruthy();
    expect(texts()).toHaveLength(0);
    expect(count()?.textContent).toBe("0 of 4");
    type("");
    expect(screen.queryByText("No lines contain that text.")).toBeNull();
  });

  // verifies: FLOW-READ-RAW-LOG-R14
  it("says that lines appear while the session runs when it holds none, filter or not", () => {
    show(viewOf([]));
    type("error");
    expect(screen.getByText("No lines yet. Lines appear here while the session runs.")).toBeTruthy();
    expect(screen.queryByText("No lines contain that text.")).toBeNull();
  });

  // verifies: FLOW-READ-RAW-LOG-R12
  it("counts nothing of the session it left while it reads the next, or with no session", () => {
    const { update } = show(viewOf(entries(0, 12)));
    type("error");
    expect(count()?.textContent).toBe("3 of 12");
    update(undefined, "s2");
    expect(count()?.hasAttribute("data-shown")).toBe(false);
    update(viewOf(entries(0, 6), { session: "s2" }), "s2");
    expect(count()?.hasAttribute("data-shown")).toBe(true);
    expect(count()?.textContent).toBe("2 of 6");
  });

  it("stays when the active session changes", () => {
    const { update } = show(viewOf(entries(0, 12)));
    type("error");
    update(viewOf(entries(0, 6), { session: "s2" }), "s2");
    expect((field() as HTMLInputElement).value).toBe("error");
    expect(texts()).toEqual(["Error 0", "Error 5"]);
  });
});
