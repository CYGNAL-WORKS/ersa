// @vitest-environment jsdom
// The developer pane's list of raw log lines (FLOW-READ-RAW-LOG, steps 2 and 3; docs/design/WORKBENCH.md, The list):
// one row a line with its time, direction and start, following the end, staying put once the user scrolls away, only
// the rows near view in the page, and what it says with nothing to list. jsdom does no layout, so the list's height is
// stubbed and the row height is the design's token, set on the page as the stylesheet would.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RawLogEntry } from "../protocol/core-api.gen";
import type { RawLogView } from "../session/rawLog";
import { rawLogTime as rawLogTimeOf } from "../session/rawLogText";
import { RawLogList } from "./RawLogList";

/** The row token's value, and the list's height: ten rows in view. */
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

const entry = (index: number, more: Partial<RawLogEntry> = {}): RawLogEntry =>
  ({ index, ts: "2026-11-22T10:00:00.000Z", dir: "backend", line: `line ${index}`, ...more }) as RawLogEntry;
/** Entries `from` up to but not including `to`. */
const entries = (from: number, to: number) => Array.from({ length: to - from }, (_, i) => entry(from + i));
const viewOf = (lines: RawLogEntry[], more: Partial<RawLogView> = {}): RawLogView => ({ session: "s1", lines, read: true, earlier: false, ...more });

type Props = Parameters<typeof RawLogList>[0];
const props = (view: RawLogView | undefined, more: Partial<Props> = {}): Props => ({ view, active: true, backend: "Claude Code", ...more });

function show(view: RawLogView | undefined, more: Partial<Props> = {}) {
  const shown = render(createElement(RawLogList, props(view, more)));
  return { ...shown, update: (next: RawLogView | undefined) => shown.rerender(createElement(RawLogList, props(next, more))) };
}

const list = () => screen.getByRole("listbox", { name: "Raw log" });
const rows = () => within(list()).queryAllByRole("option");
const rowOf = (text: string) => rows().find((row) => row.textContent?.includes(text));
/** A row's place in the list, from the variable its position is drawn by. */
const placeOf = (row: HTMLElement | undefined) => Number(row?.style.getPropertyValue("--row"));
/** The user scrolls the list to `top`. */
const scrollTo = (top: number) => {
  list().scrollTop = top;
  fireEvent.scroll(list());
};
const jump = () => screen.queryByRole("button", { name: /^Jump to latest/ });
/** Whether Jump to latest is offered: present, reachable and heard. */
const offered = () => {
  const button = jump();
  return !!button && !button.closest("[inert]") && button.closest("[aria-hidden='true']") === null;
};

describe("a row for each line", () => {
  // verifies: FLOW-READ-RAW-LOG-R03
  it("lists the lines oldest first, each with its time, its direction in words and its start", () => {
    show(
      viewOf([
        entry(0, { ts: "2026-11-22T10:00:00.007Z", dir: "host", line: '{"type":"user"}' }),
        entry(1, { ts: "2026-11-22T10:00:01.250Z", dir: "backend", line: '{"type":"assistant"}' }),
      ]),
    );
    const [first, second] = rows();
    expect(first?.textContent).toContain('{"type":"user"}');
    expect(second?.textContent).toContain('{"type":"assistant"}');
    expect(placeOf(first)).toBe(0);
    expect(placeOf(second)).toBe(1);
    // The direction is heard in words, with the backend's own label: to it, and from it.
    expect(within(first!).getByRole("img").getAttribute("aria-label")).toBe("To Claude Code");
    expect(within(second!).getByRole("img").getAttribute("aria-label")).toBe("From Claude Code");
  });

  it("tells assistive technology where each row is, though most rows aren't in the page", () => {
    show(viewOf(entries(0, 2)));
    const [first, second] = rows();
    expect(first?.getAttribute("aria-posinset")).toBe("1");
    expect(second?.getAttribute("aria-setsize")).toBe("2");
  });

  // verifies: FLOW-READ-RAW-LOG-R03
  it("gives the time in the user's local time, 24-hour, to the millisecond", () => {
    const zone = process.env.TZ;
    try {
      const line = viewOf([entry(0, { ts: "2026-11-22T20:04:05.007Z" })]);
      process.env.TZ = "UTC";
      const { unmount } = show(line);
      expect(rowOf("line 0")?.textContent).toContain("20:04:05.007");
      unmount();
      // Kolkata is five and a half hours ahead of UTC, so the hour and the minute both change.
      process.env.TZ = "Asia/Kolkata";
      show(line);
      expect(rowOf("line 0")?.textContent).toContain("01:34:05.007");
    } finally {
      if (zone === undefined) delete process.env.TZ;
      else process.env.TZ = zone;
    }
  });

  // verifies: FLOW-READ-RAW-LOG-R03
  it("shows the start of a long line, and of a line Core cut", () => {
    const long = "x".repeat(5_000);
    show(viewOf([entry(0, { line: long }), { index: 1, ts: "2026-11-22T10:00:00.000Z", dir: "backend", head: `cut ${long}`, size: 900_000 }]));
    const [whole, cut] = rows();
    const textOf = (row: HTMLElement | undefined) => row?.querySelector(".raw-log-text")?.textContent;
    // A row draws at most the line's first 2,000 characters (WORKBENCH.md, The list).
    expect(textOf(whole)).toBe("x".repeat(2_000));
    expect(textOf(cut)).toBe(`cut ${"x".repeat(1_996)}`);
  });

  it("never splits a character that takes two code units at the 2,000th", () => {
    show(viewOf([entry(0, { line: `${"x".repeat(1_999)}😀 after` })]));
    expect(rowOf("x")?.querySelector(".raw-log-text")?.textContent).toBe("x".repeat(1_999));
  });

  it("says in its first row that earlier lines are no longer held", () => {
    show(viewOf(entries(40, 42), { earlier: true }));
    const [first, second] = rows();
    expect(first?.textContent).toBe("Earlier lines are no longer held.");
    expect(placeOf(second)).toBe(1);
  });
});

describe("following the end", () => {
  // verifies: FLOW-READ-RAW-LOG-R07
  it("keeps the list at the end as lines arrive while it shows its end", () => {
    const { update } = show(viewOf(entries(0, 30)));
    // Thirty rows of 24 pixels in a list 240 high end at 480.
    expect(list().scrollTop).toBe(30 * ROW - HEIGHT);
    update(viewOf(entries(0, 35)));
    expect(list().scrollTop).toBe(35 * ROW - HEIGHT);
    expect(rowOf("line 34")).toBeDefined();
    expect(offered()).toBe(false);
  });

  // verifies: FLOW-READ-RAW-LOG-R07
  it("once the user scrolls away, leaves the rows in view where they are, counts the new lines and offers Jump to latest", () => {
    const { update } = show(viewOf(entries(0, 30)));
    scrollTo(100);
    // Away from the end, with nothing new yet: Jump to latest is offered without a count.
    expect(offered()).toBe(true);
    expect(jump()?.getAttribute("aria-label")).toBe("Jump to latest");
    update(viewOf(entries(0, 33)));
    expect(list().scrollTop).toBe(100);
    expect(jump()?.textContent).toBe("Jump to latest3 new");
    expect(jump()?.getAttribute("aria-label")).toBe("Jump to latest, 3 new lines");
    update(viewOf(entries(0, 34)));
    expect(jump()?.getAttribute("aria-label")).toBe("Jump to latest, 4 new lines");
  });

  // verifies: FLOW-READ-RAW-LOG-R07
  it("keeps the line the user reads in place when lines go from the front and the earlier-lines row comes", () => {
    const { update } = show(viewOf(entries(0, 30)));
    // Row 4, four pixels down: line 4 is at the top of the view.
    scrollTo(4 * ROW + 4);
    update(viewOf(entries(2, 32), { earlier: true }));
    const top = rowOf("line 4");
    // Line 4 is now the fourth row (the earlier-lines row, then lines 2 and 3), still four pixels down.
    expect(placeOf(top)).toBe(3);
    expect(list().scrollTop).toBe(3 * ROW + 4);
    expect(jump()?.getAttribute("aria-label")).toBe("Jump to latest, 2 new lines");
  });

  // verifies: FLOW-READ-RAW-LOG-R07
  it("returns to the end and to following with Jump to latest", () => {
    const { update } = show(viewOf(entries(0, 30)));
    scrollTo(0);
    update(viewOf(entries(0, 32)));
    fireEvent.click(jump()!);
    expect(list().scrollTop).toBe(32 * ROW - HEIGHT);
    expect(offered()).toBe(false);
    update(viewOf(entries(0, 40)));
    expect(list().scrollTop).toBe(40 * ROW - HEIGHT);
    expect(offered()).toBe(false);
  });

  // verifies: FLOW-READ-RAW-LOG-R07
  it("follows again once the user scrolls back to the end, and counts from nothing the next time", () => {
    const { update } = show(viewOf(entries(0, 30)));
    scrollTo(0);
    update(viewOf(entries(0, 31)));
    scrollTo(31 * ROW - HEIGHT);
    expect(offered()).toBe(false);
    update(viewOf(entries(0, 36)));
    expect(list().scrollTop).toBe(36 * ROW - HEIGHT);
    scrollTo(0);
    expect(jump()?.getAttribute("aria-label")).toBe("Jump to latest");
  });

  // verifies: FLOW-READ-RAW-LOG-R07
  it("follows the end again when the reader starts over, as after connecting to Core again", () => {
    const { update } = show(viewOf(entries(0, 30)));
    scrollTo(0);
    update(viewOf(entries(0, 33)));
    expect(offered()).toBe(true);
    // The reader lets go of what it held and reads from Core's first line again: a Core that restarted numbers its
    // lines from 0 again, so nothing of where the user was reading still holds.
    update(viewOf([], { read: false }));
    update(viewOf(entries(0, 40)));
    expect(list().scrollTop).toBe(40 * ROW - HEIGHT);
    expect(offered()).toBe(false);
    update(viewOf(entries(0, 45)));
    expect(list().scrollTop).toBe(45 * ROW - HEIGHT);
    // Scrolled away again, the count starts from the lines held now.
    scrollTo(0);
    update(viewOf(entries(0, 47)));
    expect(jump()?.getAttribute("aria-label")).toBe("Jump to latest, 2 new lines");
  });

  // verifies: FLOW-READ-RAW-LOG-R20
  it("sends keyboard focus that was on Jump to latest to the list as it goes", () => {
    show(viewOf(entries(0, 30)));
    scrollTo(0);
    jump()!.focus();
    act(() => jump()!.click());
    expect(document.activeElement).toBe(list());
  });
});

describe("only the rows near view in the page", () => {
  // verifies: FLOW-READ-RAW-LOG-R08
  it("keeps in the page only the rows in or near view, however many lines it holds", () => {
    show(viewOf(entries(0, 50_000)));
    // At the end: the last ten rows are in view, and a view's height of rows either side is near it.
    const near = rows();
    expect(near.length).toBeLessThanOrEqual(30);
    expect(rowOf("line 49999")).toBeDefined();
    expect(rowOf("line 49989")).toBeDefined();
    expect(rowOf("line 40000")).toBeUndefined();
    // The list is as tall as every row, so it scrolls as far as the lines go.
    expect(list().querySelector<HTMLElement>(".raw-log-rows")?.style.getPropertyValue("--rows")).toBe("50000");

    scrollTo(25_000 * ROW);
    expect(rows().length).toBeLessThanOrEqual(31);
    expect(placeOf(rowOf("line 25000"))).toBe(25_000);
    expect(rowOf("line 25009")).toBeDefined();
    expect(rowOf("line 49999")).toBeUndefined();
  });
});

describe("nothing to list", () => {
  // verifies: FLOW-READ-RAW-LOG-R14
  it("says to choose a session when there is no active session", () => {
    show(undefined, { active: false });
    expect(screen.getByText("No session is open. Choose a session to see its raw log.")).toBeTruthy();
    expect(rows()).toHaveLength(0);
  });

  // verifies: FLOW-READ-RAW-LOG-R14
  it("says it is reading until the first answer comes, then that lines appear while the session runs", () => {
    const { update } = show(viewOf([], { read: false }));
    expect(screen.getByText("Reading…")).toBeTruthy();
    expect(screen.queryByText("No lines yet. Lines appear here while the session runs.")).toBeNull();
    update(viewOf([]));
    expect(screen.queryByText("Reading…")).toBeNull();
    expect(screen.getByText("No lines yet. Lines appear here while the session runs.")).toBeTruthy();
    update(viewOf(entries(0, 1)));
    expect(screen.queryByText("No lines yet. Lines appear here while the session runs.")).toBeNull();
    expect(rows()).toHaveLength(1);
  });
});

describe("while the workbench closes", () => {
  it("keeps the rows it showed when the reader lets go, rather than a message", () => {
    const { update } = show(viewOf(entries(0, 3)));
    update(undefined);
    expect(rows()).toHaveLength(3);
    expect(screen.queryByText("Reading…")).toBeNull();
  });
});

describe("a read that failed", () => {
  it("says so with the reason over the list, keeps the rows, and the message goes when a read succeeds", () => {
    const { update } = show(viewOf(entries(0, 3), { failure: "Core isn't running." }));
    const problem = document.querySelector(".raw-log-problem")!;
    expect(problem.textContent).toBe("Ersa can't read the raw log. It's trying again.Core isn't running.");
    expect(problem.getAttribute("data-shown")).toBe("true");
    // Assistive technology hears it once, from a status region.
    expect(screen.getByRole("status").textContent).toBe("Ersa can't read the raw log. It's trying again. Core isn't running.");
    expect(rows()).toHaveLength(3);
    update(viewOf(entries(0, 3)));
    expect(problem.getAttribute("data-shown")).toBeNull();
    expect(screen.getByRole("status").textContent).toBe("");
  });
});

describe("a selected line in the detail", () => {
  let written: string[];
  let take: (text: string) => Promise<void>;
  beforeEach(() => {
    written = [];
    take = (text) => (written.push(text), Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: (text: string) => take(text) } });
  });

  const detail = () => document.querySelector<HTMLElement>(".raw-log-detail")!;
  const detailOpen = () => detail().hasAttribute("data-shown") && !detail().inert;
  const shownText = () => detail().querySelector("pre")?.textContent;
  const copyButton = () => within(detail()).getByRole("button", { name: /^Copy line$|^Copied$/ });
  const clickCopy = () =>
    act(async () => {
      fireEvent.click(copyButton());
    });

  // verifies: FLOW-READ-RAW-LOG-R10
  it("shows the selected line beneath the list, indented as JSON, with its direction, time and size", () => {
    const json = '{"type":"user","message":{"content":"hi"},"n":1.0}';
    show(viewOf([entry(0), entry(1, { dir: "host", line: json, ts: "2026-11-22T10:00:01.250Z" })]));
    expect(detailOpen()).toBe(false);
    fireEvent.click(rowOf('"type"')!);
    expect(detailOpen()).toBe(true);
    expect(rowOf('"type"')?.getAttribute("aria-selected")).toBe("true");
    expect(rowOf("line 0")?.getAttribute("aria-selected")).toBe("false");
    expect(shownText()).toBe(["{", '  "type": "user",', '  "message": {', '    "content": "hi"', "  },", '  "n": 1.0', "}"].join("\n"));
    const head = detail().querySelector(".raw-log-detail-head")!.textContent;
    expect(head).toContain("To Claude Code");
    expect(head).toContain(rawLogTimeOf("2026-11-22T10:00:01.250Z"));
    expect(head).toContain(`${json.length} bytes`);
  });

  // verifies: FLOW-READ-RAW-LOG-R10
  it("shows a line that is not valid JSON exactly as it crossed", () => {
    const line = '  Claude Code started {"half":';
    show(viewOf([entry(0, { line })]));
    fireEvent.click(rowOf("Claude Code started")!);
    expect(shownText()).toBe(line);
  });

  // verifies: FLOW-READ-RAW-LOG-R10
  it("copies the line as it crossed, not as it is laid out, and says so only once the clipboard has it", async () => {
    const json = '{"type":"user","n":1e3}';
    show(viewOf([entry(0, { line: json })]));
    fireEvent.click(rowOf('"type"')!);
    let release!: () => void;
    take = (text) => (written.push(text), new Promise<void>((resolve) => (release = resolve)));
    await clickCopy();
    expect(written).toEqual([json]);
    // The write is not done yet: Copy says nothing of it.
    expect(copyButton().getAttribute("aria-label")).toBe("Copy line");
    await act(async () => release());
    expect(copyButton().getAttribute("aria-label")).toBe("Copied");
  });

  // verifies: FLOW-READ-RAW-LOG-R10
  it("doesn't say it copied when the clipboard refuses the line", async () => {
    take = () => Promise.reject(new DOMException("Write permission denied.", "NotAllowedError"));
    show(viewOf([entry(0)]));
    fireEvent.click(rowOf("line 0")!);
    await clickCopy();
    expect(copyButton().getAttribute("aria-label")).toBe("Copy line");
  });

  // verifies: FLOW-READ-RAW-LOG-R11
  it("for a line Core cut, says how large the whole line is, shows its start unchanged and copies only that", async () => {
    // The start happens to be JSON on its own; a cut line is still shown as it came.
    const head = '{"type":"assistant"}';
    show(viewOf([{ index: 0, ts: "2026-11-22T10:00:00.000Z", dir: "backend", head, size: 1_843_200 }]));
    fireEvent.click(rowOf('"assistant"')!);
    expect(detail().querySelector(".raw-log-detail-note")?.textContent).toBe("This line is 1.8 MB. Its first 256 KB are shown.");
    expect(detail().querySelector(".raw-log-detail-head")?.textContent).toContain("1.8 MB");
    expect(shownText()).toBe(head);
    await clickCopy();
    expect(written).toEqual([head]);
  });

  it("says nothing of a cut for a whole line", () => {
    show(viewOf([entry(0)]));
    fireEvent.click(rowOf("line 0")!);
    expect(detail().querySelector(".raw-log-detail-note")).toBeNull();
  });

  // verifies: FLOW-READ-RAW-LOG-R10
  it("closes with Close line, and keyboard focus inside it goes to the list", () => {
    show(viewOf(entries(0, 3)));
    fireEvent.click(rowOf("line 1")!);
    const close = within(detail()).getByRole("button", { name: "Close line" });
    close.focus();
    fireEvent.click(close);
    expect(detailOpen()).toBe(false);
    expect(rowOf("line 1")?.getAttribute("aria-selected")).toBe("false");
    expect(document.activeElement).toBe(list());
  });

  // verifies: FLOW-READ-RAW-LOG-R10
  it("closes with Escape from inside it, and keyboard focus goes to the list", () => {
    show(viewOf(entries(0, 3)));
    fireEvent.click(rowOf("line 1")!);
    copyButton().focus();
    fireEvent.keyDown(copyButton(), { key: "Escape" });
    expect(detailOpen()).toBe(false);
    expect(document.activeElement).toBe(list());
  });

  it("clears the selection when the reader starts over", () => {
    const { update } = show(viewOf(entries(0, 3)));
    fireEvent.click(rowOf("line 1")!);
    update(viewOf([], { read: false }));
    update(viewOf(entries(0, 3)));
    expect(detailOpen()).toBe(false);
    expect(rows().every((row) => row.getAttribute("aria-selected") !== "true")).toBe(true);
  });
});

describe("the keyboard", () => {
  const selectedText = () => rows().find((row) => row.getAttribute("aria-selected") === "true")?.querySelector(".raw-log-text")?.textContent;
  const press = (key: string) => fireEvent.keyDown(list(), { key });
  const detailShown = () => document.querySelector(".raw-log-detail")?.hasAttribute("data-shown");

  // verifies: FLOW-READ-RAW-LOG-R17
  it("takes keyboard focus as one stop: the list, never a row", () => {
    show(viewOf(entries(0, 3)));
    expect(list().tabIndex).toBe(0);
    expect(rows().every((row) => row.tabIndex < 0)).toBe(true);
  });

  // verifies: FLOW-READ-RAW-LOG-R17
  it("moves the selection a row with Up and Down, and to the first and last row with Home and End", () => {
    show(viewOf(entries(0, 30)));
    // Following the end: rows 20 to 29 are in view, so Down with nothing selected takes the first of them.
    press("ArrowDown");
    expect(selectedText()).toBe("line 20");
    press("ArrowDown");
    expect(selectedText()).toBe("line 21");
    press("ArrowUp");
    expect(selectedText()).toBe("line 20");
    press("Home");
    expect(selectedText()).toBe("line 0");
    // Home brought the first row into view, away from the end.
    expect(list().scrollTop).toBe(0);
    expect(offered()).toBe(true);
    press("ArrowUp");
    expect(selectedText()).toBe("line 0");
    press("End");
    expect(selectedText()).toBe("line 29");
    // End returns to the end and to following.
    expect(list().scrollTop).toBe(30 * ROW - HEIGHT);
    expect(offered()).toBe(false);
    press("ArrowDown");
    expect(selectedText()).toBe("line 29");
  });

  // verifies: FLOW-READ-RAW-LOG-R17
  it("takes the last row in view with Up when nothing is selected, and names the selected row to assistive technology", () => {
    show(viewOf(entries(0, 30)));
    press("ArrowUp");
    expect(selectedText()).toBe("line 29");
    expect(list().getAttribute("aria-activedescendant")).toBe(rowOf("line 29")?.id);
  });

  // verifies: FLOW-READ-RAW-LOG-R17
  it("clears the selection and closes the detail with Escape, leaving focus in the list", () => {
    show(viewOf(entries(0, 3)));
    list().focus();
    press("ArrowDown");
    expect(detailShown()).toBe(true);
    press("Escape");
    expect(selectedText()).toBeUndefined();
    expect(detailShown()).toBe(false);
    expect(document.activeElement).toBe(list());
  });

  it("keeps keyboard focus on the list when the reader starts over and a message takes the rows' place", () => {
    const { update } = show(viewOf(entries(0, 3)));
    list().focus();
    update(viewOf([], { read: false }));
    expect(screen.getByText("Reading…")).toBeTruthy();
    expect(document.activeElement).toBe(list());
  });
});
