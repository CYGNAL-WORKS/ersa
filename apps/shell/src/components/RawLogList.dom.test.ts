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

const list = () => screen.getByRole("list", { name: "Raw log" });
const rows = () => within(list()).queryAllByRole("listitem");
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
    // Each row is one of how many, so assistive technology hears where it is though most rows aren't in the page.
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
    expect(screen.queryByRole("list")).toBeNull();
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
