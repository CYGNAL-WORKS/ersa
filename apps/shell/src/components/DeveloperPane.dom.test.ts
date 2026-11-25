// @vitest-environment jsdom
// The developer pane's toolbar over its list (FLOW-READ-RAW-LOG, step 5; docs/design/WORKBENCH.md, The developer
// pane): the filter field lists only the lines that contain its text, whatever its case, says how many of the lines
// held those are, and goes on following the end. jsdom does no layout, so the list's height is stubbed and the row
// height is the design's token, set on the page as the stylesheet would.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
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
const selectedText = () =>
  within(list())
    .queryAllByRole("option")
    .find((row) => row.getAttribute("aria-selected") === "true")
    ?.querySelector(".raw-log-text")?.textContent;
const detailOpen = () => {
  const detail = document.querySelector<HTMLElement>(".raw-log-detail");
  return !!detail?.hasAttribute("data-shown") && !detail.inert;
};

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

  // verifies: FLOW-READ-RAW-LOG-R21
  it("keeps a selected line selected, its detail open, while the filter leaves it out", () => {
    show(viewOf(entries(0, 12)));
    fireEvent.click(within(list()).getByText("ok 3"));
    type("error");
    expect(detailOpen()).toBe(true);
    expect(document.querySelector(".raw-log-detail pre")?.textContent).toBe("ok 3");
    type("");
    expect(selectedText()).toBe("ok 3");
  });

  // verifies: FLOW-READ-RAW-LOG-R24
  it("moves from a selected line the list doesn't show to the nearest listed line, the way the key points", () => {
    show(viewOf(entries(0, 12)));
    const press = (key: string) => fireEvent.keyDown(list(), { key });
    const from = (text: string, filter: string) => {
      type("");
      fireEvent.click(within(list()).getByText(text));
      type(filter);
    };
    // Lines 0, 5 and 10 hold "Error"; line 3 is selected and left out.
    from("ok 3", "error");
    press("ArrowDown");
    expect(selectedText()).toBe("Error 5");
    from("ok 3", "error");
    press("ArrowUp");
    expect(selectedText()).toBe("Error 0");
    // With no listed line on the key's side, the nearest listed line.
    from("ok 11", "error");
    press("ArrowDown");
    expect(selectedText()).toBe("Error 10");
    type("");
    fireEvent.click(within(list()).getByText("Error 0"));
    type("ok");
    press("ArrowUp");
    expect(selectedText()).toBe("ok 1");
  });

  it("stays when the active session changes", () => {
    const { update } = show(viewOf(entries(0, 12)));
    type("error");
    update(viewOf(entries(0, 6), { session: "s2" }), "s2");
    expect((field() as HTMLInputElement).value).toBe("error");
    expect(texts()).toEqual(["Error 0", "Error 5"]);
  });
});

describe("Open diagnostic log", () => {
  const button = () => screen.getByRole("button", { name: "Open diagnostic log" });
  // The second problem line, which says the diagnostic log couldn't be opened (R25).
  const problem = () => document.querySelectorAll(".raw-log-problem")[1];
  const opening = (open: () => Promise<void>, attachedTo?: string) =>
    render(createElement(DeveloperPane, { session: "s1", view: viewOf(entries(0, 3)), backend: "Claude Code", onOpenDiagnosticLog: open, attachedTo }));
  const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));

  // verifies: FLOW-READ-RAW-LOG-R13
  it("asks the Shell to open the diagnostic log, and says nothing when it opened", async () => {
    const open = vi.fn(() => Promise.resolve());
    opening(open);
    expect(button().getAttribute("aria-disabled")).toBeNull();
    fireEvent.click(button());
    await settle();
    expect(open).toHaveBeenCalledTimes(1);
    expect(problem()?.hasAttribute("data-shown")).toBe(false);
  });

  // verifies: FLOW-READ-RAW-LOG-R13
  it("says so with the reason when the diagnostic log couldn't be opened, until it opens", async () => {
    const reason = "There's no diagnostic log at C:\Users\a\AppData\Local\Ersa\core.log yet.";
    const open = vi.fn<() => Promise<void>>(() => Promise.reject(reason));
    opening(open);
    fireEvent.click(button());
    await settle();
    expect(problem()?.hasAttribute("data-shown")).toBe(true);
    expect(problem()?.querySelector(".raw-log-problem-message")?.textContent).toBe("Ersa couldn't open the diagnostic log.");
    expect(problem()?.querySelector(".raw-log-problem-reason")?.textContent).toBe(reason);
    expect(problem()?.getAttribute("title")).toBe(`Ersa couldn't open the diagnostic log. ${reason}`);
    expect(screen.getAllByRole("status").map((s) => s.textContent)).toContain(`Ersa couldn't open the diagnostic log. ${reason}`);
    open.mockImplementation(() => Promise.resolve());
    fireEvent.click(button());
    await settle();
    expect(problem()?.hasAttribute("data-shown")).toBe(false);
  });

  // verifies: FLOW-READ-RAW-LOG-R13
  it("is unavailable in a build attached to a labeled endpoint, and says why", async () => {
    const open = vi.fn(() => Promise.resolve());
    opening(open, "replay-7");
    expect(button().getAttribute("aria-disabled")).toBe("true");
    const why = "This build is attached to replay-7, so it has no diagnostic log to open.";
    expect(button().getAttribute("title")).toBe(why);
    expect(button().getAttribute("aria-description") ?? document.getElementById(button().getAttribute("aria-describedby") ?? "")?.textContent).toBe(why);
    fireEvent.click(button());
    await settle();
    expect(open).not.toHaveBeenCalled();
  });
});

describe("what the pane says about the diagnostic log", () => {
  const reason = "There's no diagnostic log at D:\\data\\core.log yet.";
  const lines = () => [...document.querySelectorAll(".raw-log-problem")];
  const shownLines = () => lines().filter((line) => line.hasAttribute("data-shown"));
  const message = (line: Element) => line.querySelector(".raw-log-problem-message")?.textContent;
  const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));
  const props = (more: Partial<Parameters<typeof DeveloperPane>[0]> = {}) => ({
    session: "s1",
    view: viewOf(entries(0, 3)),
    backend: "Claude Code",
    onOpenDiagnosticLog: () => Promise.reject(reason),
    ...more,
  });
  async function failed(more: Partial<Parameters<typeof DeveloperPane>[0]> = {}) {
    const shown = render(createElement(DeveloperPane, props(more)));
    fireEvent.click(screen.getByRole("button", { name: "Open diagnostic log" }));
    await settle();
    return { rerender: (next: Partial<Parameters<typeof DeveloperPane>[0]>) => shown.rerender(createElement(DeveloperPane, props({ ...more, ...next }))) };
  }

  // verifies: FLOW-READ-RAW-LOG-R25
  it("says both problems while both stand, the read's first and the open's beneath it", async () => {
    await failed({ view: viewOf(entries(0, 3), { failure: "Core isn't running." }) });
    const [read, open] = shownLines();
    expect(shownLines()).toHaveLength(2);
    expect(message(read!)).toBe("Ersa can't read the raw log. It's trying again.");
    expect(read!.hasAttribute("data-below")).toBe(false);
    expect(message(open!)).toBe("Ersa couldn't open the diagnostic log.");
    expect(open!.hasAttribute("data-below")).toBe(true);
    const heard = screen.getAllByRole("status").map((s) => s.textContent);
    expect(heard).toContain("Ersa can't read the raw log. It's trying again. Core isn't running.");
    expect(heard).toContain(`Ersa couldn't open the diagnostic log. ${reason}`);
  });

  // verifies: FLOW-READ-RAW-LOG-R25
  it("keeps the open's line when a read starts failing after it, and at the top while it stands alone", async () => {
    const { rerender } = await failed();
    expect(shownLines()).toHaveLength(1);
    expect(shownLines()[0]!.hasAttribute("data-below")).toBe(false);
    rerender({ view: viewOf(entries(0, 3), { failure: "Core isn't running." }) });
    expect(shownLines().map(message)).toEqual(["Ersa can't read the raw log. It's trying again.", "Ersa couldn't open the diagnostic log."]);
  });

  // verifies: FLOW-READ-RAW-LOG-R26
  it("stays until the user's next key in the list, and goes", async () => {
    await failed();
    // Lines arriving and time passing don't end it.
    await settle();
    expect(shownLines()).toHaveLength(1);
    fireEvent.keyDown(list(), { key: "ArrowDown" });
    expect(shownLines()).toHaveLength(0);
  });

  // verifies: FLOW-READ-RAW-LOG-R26
  it("goes at the user's next press in the toolbar, and a read's failure stays", async () => {
    await failed({ view: viewOf(entries(0, 3), { failure: "Core isn't running." }) });
    fireEvent.pointerDown(field());
    expect(shownLines().map(message)).toEqual(["Ersa can't read the raw log. It's trying again."]);
  });

  // verifies: FLOW-READ-RAW-LOG-R26
  it("goes at the user's next press in the detail", async () => {
    render(createElement(DeveloperPane, props()));
    fireEvent.click(within(list()).getByText("ok 1"));
    // The choice fails after the line is selected; then a press inside the detail ends what it said.
    fireEvent.click(screen.getByRole("button", { name: "Open diagnostic log" }));
    await settle();
    expect(shownLines()).toHaveLength(1);
    fireEvent.pointerDown(screen.getByRole("button", { name: "Copy line" }));
    expect(shownLines()).toHaveLength(0);
  });

  // verifies: FLOW-READ-RAW-LOG-R26
  it("goes when the active session changes, and when the workbench closes", async () => {
    const { rerender } = await failed();
    rerender({ session: "s2", view: viewOf(entries(0, 3), { session: "s2" }) });
    expect(shownLines()).toHaveLength(0);
    cleanup();
    const again = await failed();
    expect(shownLines()).toHaveLength(1);
    again.rerender({ open: false });
    expect(shownLines()).toHaveLength(0);
  });

  // verifies: FLOW-READ-RAW-LOG-R26
  it("says it again when the next choice fails again", async () => {
    await failed();
    const button = screen.getByRole("button", { name: "Open diagnostic log" });
    fireEvent.pointerDown(button);
    fireEvent.click(button);
    await settle();
    expect(shownLines()).toHaveLength(1);
  });
});

describe("the toolbar's rows", () => {
  const TITLE_MIN = 120;
  const GAP = 8;
  const BUTTON = 118;
  function at(width: number) {
    document.documentElement.style.setProperty("--size-row-title-min", `${TITLE_MIN}px`);
    document.documentElement.style.setProperty("--space-sm", `${GAP}px`);
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(function (this: HTMLElement) {
      return this.classList.contains("developer-toolbar") ? width : 0;
    });
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockImplementation(function (this: HTMLElement) {
      return this.classList.contains("developer-open-log") ? BUTTON : 0;
    });
    render(createElement(DeveloperPane, { session: "s1", view: viewOf(entries(0, 3)), backend: "Claude Code" }));
    return document.querySelector(".developer-toolbar")!;
  }

  it("is one row while it can give the field and the count twice size-row-title-min beside the button", () => {
    expect(at(2 * TITLE_MIN + GAP + BUTTON).hasAttribute("data-narrow")).toBe(false);
  });

  it("puts the button on a second row when narrower, whatever is typed", () => {
    const toolbar = at(2 * TITLE_MIN + GAP + BUTTON - 1);
    expect(toolbar.hasAttribute("data-narrow")).toBe(true);
    type("a much longer filter than the field can show");
    expect(toolbar.hasAttribute("data-narrow")).toBe(true);
    type("");
    expect(toolbar.hasAttribute("data-narrow")).toBe(true);
  });
});
