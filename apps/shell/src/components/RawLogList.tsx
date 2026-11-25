import { ArrowDown, ArrowDownLeft, ArrowUpRight } from "lucide-react";
import { type CSSProperties, type KeyboardEvent, type UIEvent, useCallback, useId, useLayoutEffect, useRef, useState } from "react";
import { t } from "../catalog";
import type { RawLogEntry } from "../protocol/core-api.gen";
import type { RawLogView } from "../session/rawLog";
import { lineText, rawLogTime } from "../session/rawLogText";
import { Icon } from "./Icon";
import { RawLogDetail } from "./RawLogDetail";
import "./RawLogList.css";

/** A row draws at most this many characters of its line (docs/design/WORKBENCH.md, The list). */
const ROW_CHARACTERS = 2_000;

/** The start a row draws: the line, or the start Core sent of a cut one, to at most ROW_CHARACTERS characters. */
function rowText(entry: RawLogEntry): string {
  const text = lineText(entry);
  if (text.length <= ROW_CHARACTERS) return text;
  const start = text.slice(0, ROW_CHARACTERS);
  // A character outside the Basic Multilingual Plane takes two code units; never keep the first without the second.
  const last = start.charCodeAt(start.length - 1);
  return last >= 0xd800 && last <= 0xdbff ? start.slice(0, -1) : start;
}

/** The position of the first held line whose index is above `index`; the lines are in index order. */
function firstAfter(lines: readonly RawLogEntry[], index: number): number {
  let low = 0;
  let high = lines.length;
  while (low < high) {
    const mid = (low + high) >> 1;
    if (lines[mid]!.index > index) high = mid;
    else low = mid + 1;
  }
  return low;
}

/** The list's own measure: the row token, in pixels, and the list's height. */
interface Measure {
  row: number;
  height: number;
}

/**
 * Where the user reads while away from the end: the line at the top of the view, by its index (-1 for the
 * earlier-lines row), and how far into it the view starts. Kept by index, not by position, so rows in view stay where
 * they are when lines go from the front or the earlier-lines row comes (FLOW-READ-RAW-LOG-R07).
 */
interface Anchor {
  index: number;
  offset: number;
}

/**
 * The developer pane's list of the active session's raw log (FLOW-READ-RAW-LOG, steps 2 to 4; docs/design/WORKBENCH.md,
 * The list and The detail): a row a line, oldest first, following the end as lines arrive until the user scrolls away;
 * then the rows in view stay put and Jump to latest, counting the lines that arrived since, takes them back. Only the
 * rows in or near view are in the page (R08). A selected row's line shows whole in the detail beneath (R10); the list
 * is one stop for the keyboard, and its keys move the selection (R17). With nothing to list it says why (R14), and a
 * read that failed lies over the top of the list as one line (R15). Keyed by session where it is used, so a new
 * session starts at its end, following, with no row selected (R04).
 */
export function RawLogList({
  view: given,
  lines: givenLines,
  filtering = false,
  active,
  backend,
  unopened,
  force,
}: {
  /** What the reader holds of the active session's raw log; undefined before it has read anything. */
  view: RawLogView | undefined;
  /** The lines to list, when the filter leaves out some of those held (R12); all of them otherwise. */
  lines?: RawLogEntry[];
  /** Whether the filter field holds text, so a list with no row says that no line contains it (R14). */
  filtering?: boolean;
  /** Whether there is an active session: none while the new-session screen shows. */
  active: boolean;
  /** The session's backend, by its own label, for the direction assistive technology hears. */
  backend: string;
  /**
   * Why the diagnostic log couldn't be opened, after the user last chose Open diagnostic log and it failed
   * (FLOW-READ-RAW-LOG-R13), said on a line of its own (R25).
   */
  unopened?: string;
  /**
   * For the gallery: scrolled away with `away` new lines, Jump to latest in a pointer or keyboard state, a row under
   * the pointer, a line selected by its index, the list with keyboard focus, and the detail's buttons in a state.
   */
  force?: {
    away?: number;
    jump?: "hover" | "pressed" | "focus";
    hover?: number;
    selected?: number;
    focus?: boolean;
    detail?: Parameters<typeof RawLogDetail>[0]["force"];
  };
}) {
  const root = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const ids = useId();
  // The reader reports nothing while the workbench closes, and the list runs back with what it showed, not a message.
  const [kept, setKept] = useState({ view: given, lines: givenLines });
  if (given !== undefined && (given !== kept.view || givenLines !== kept.lines)) setKept({ view: given, lines: givenLines });
  const view = given ?? (active ? kept.view : undefined);
  const lines = (given ? givenLines : kept.lines) ?? view?.lines ?? [];
  const earlier = view?.earlier ?? false;
  // The earlier-lines row, when there is one and any line is listed, is the list's first row; line n is then row n + 1.
  const lead = earlier && lines.length > 0 ? 1 : 0;
  const total = lead + lines.length;

  const [measure, setMeasure] = useState<Measure>({ row: 0, height: 0 });
  const [top, setTop] = useState(0);
  // Following the end, as it does until the user scrolls away from it.
  const [following, setFollowing] = useState(force?.away === undefined);
  // The last index held when the user left the end; lines above it arrived since.
  const [seen, setSeen] = useState(() => (force?.away === undefined ? -1 : (lines.at(-1)?.index ?? 0) - force.away));
  const anchor = useRef<Anchor>({ index: -1, offset: 0 });
  // The selected line, followed by its index, so it stays selected while lines come and go around it.
  const [selected, setSelected] = useState(() => lines.find((line) => line.index === force?.selected));
  // Set when the selected row should come into view: it was just chosen, or the list's height changed under it.
  const reveal = useRef(selected !== undefined);
  // The reader starting over (FLOW-READ-RAW-LOG-R18: Ersa connected to Core again, or the pane opened again while it
  // ran back) reports a view with nothing held and nothing read yet. Where the user read, what they had seen and what
  // they selected no longer hold, since a Core that restarted numbers its lines from 0 again: the list follows the end
  // again (R07) with no row selected.
  const startOver = given !== undefined && !given.read && given.lines.length === 0;
  const [wasStartOver, setWasStartOver] = useState(startOver);
  if (startOver !== wasStartOver) {
    setWasStartOver(startOver);
    if (startOver) {
      setFollowing(true);
      setSeen(-1);
      setSelected(undefined);
    }
  }

  // Where the selected line is among the lines listed; `listed` is false when the filter leaves it out.
  const selectedAt = selected === undefined ? -1 : firstAfter(lines, selected.index - 1);
  const listed = selected !== undefined && lines[selectedAt]?.index === selected.index;

  /** The line at the top of a view starting at `scrollTop`, as an anchor. */
  const anchorAt = useCallback(
    (scrollTop: number): Anchor => {
      if (measure.row <= 0) return { index: -1, offset: 0 };
      const row = Math.floor(scrollTop / measure.row);
      const line = lines[row - lead];
      return { index: line?.index ?? -1, offset: scrollTop - row * measure.row };
    },
    [lines, lead, measure.row],
  );

  // The row token and the list's height, read where the list is drawn, and again whenever its size changes, as it
  // does while the detail grows in beneath it.
  useLayoutEffect(() => {
    const el = list.current;
    if (!el) return;
    const read = () => {
      const row = parseFloat(getComputedStyle(el).getPropertyValue("--size-log-row"));
      setMeasure((was) => {
        if (was.row === row && was.height === el.clientHeight) return was;
        // The detail coming or going changes the list's height: the selected row stays in view (WORKBENCH.md, The detail).
        if (was.height !== el.clientHeight) reveal.current = true;
        return { row, height: el.clientHeight };
      });
    };
    read();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(read);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // After each change of what the list holds, of its size or of the selection: at the end while following, new rows
  // appearing at once (WORKBENCH.md, New rows); else the line the user reads stays where it was in the view. A row just
  // selected, or one the list's height changed under, then comes into view, which may leave the end or return to it.
  useLayoutEffect(() => {
    const el = list.current;
    if (!el || measure.row <= 0) return;
    const end = Math.max(0, total * measure.row - measure.height);
    let next: number;
    if (following) next = end;
    else {
      const { index, offset } = anchor.current;
      // The earlier-lines row is row 0. The line at the top may have gone from the front, or the filter may have left
      // it out: the nearest line after it takes its place, or the top when none is left before it (R12 goes on as R07).
      const after = index < 0 ? 0 : firstAfter(lines, index - 1);
      const exact = index < 0 || lines[after]?.index === index;
      next = Math.min(end, exact ? (index < 0 ? offset : (lead + after) * measure.row + offset) : after === 0 ? 0 : (lead + after) * measure.row);
    }
    if (reveal.current && listed) {
      const rowTop = (lead + selectedAt) * measure.row;
      next = Math.max(Math.min(next, rowTop), rowTop + measure.row - measure.height);
      next = Math.max(0, Math.min(end, next));
      // A row brought into view away from the end stops the list following, as scrolling there would; End's row
      // brings it back to the end and to following.
      const atEnd = next + measure.height >= total * measure.row - measure.row;
      if (atEnd !== following) {
        setFollowing(atEnd);
        if (!atEnd) setSeen(lines.at(-1)?.index ?? -1);
      }
    }
    reveal.current = false;
    if (el.scrollTop !== next) el.scrollTop = next;
    anchor.current = anchorAt(next);
    setTop(next);
  }, [lines, lead, total, measure, following, anchorAt, listed, selectedAt]);

  // Keyboard focus never falls to the window when the detail closes with it inside: it goes to the list (R10).
  useLayoutEffect(() => {
    if (selected !== undefined) return;
    const detail = root.current?.querySelector(".raw-log-detail");
    if (detail?.contains(document.activeElement)) list.current?.focus({ preventScroll: true });
  }, [selected]);

  const onScroll = (event: UIEvent<HTMLDivElement>) => {
    const scrollTop = event.currentTarget.scrollTop;
    setTop(scrollTop);
    anchor.current = anchorAt(scrollTop);
    // The list shows its end while its last row is in view, within a row, so a part-row of scrolling still follows.
    const atEnd = scrollTop + measure.height >= total * measure.row - measure.row;
    if (atEnd !== following) {
      setFollowing(atEnd);
      if (!atEnd) setSeen(lines.at(-1)?.index ?? -1);
    }
  };

  // Jump to latest: back to the end at once, and to following. Keyboard focus on it, which is about to go, goes to
  // the list, never to the window (R20).
  const jumpToLatest = (event: { currentTarget: HTMLElement }) => {
    if (event.currentTarget === document.activeElement) list.current?.focus({ preventScroll: true });
    setFollowing(true);
  };

  /** Selects a line and brings its row into view. */
  const select = (line: RawLogEntry | undefined) => {
    if (line === undefined) return;
    reveal.current = true;
    setSelected(line);
  };

  // R17: Up and Down move the selection by a row, Home and End to the first and last row, and Escape clears it and
  // closes the detail, leaving focus in the list. With no row selected, Down takes the first row wholly in view and
  // Up the last, so the reader stays where they are. A selected line the filter leaves out moves to its neighbor.
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      if (selected === undefined) return;
      event.preventDefault();
      setSelected(undefined);
      return;
    }
    if (lines.length === 0 || measure.row <= 0) return;
    const last = lines.length - 1;
    const firstInView = Math.min(last, Math.max(0, Math.ceil(top / measure.row) - lead));
    const lastInView = Math.max(0, Math.min(last, Math.floor((top + measure.height) / measure.row) - 1 - lead));
    const to: Record<string, () => number> = {
      ArrowDown: () => (selected === undefined ? firstInView : Math.min(last, listed ? selectedAt + 1 : selectedAt)),
      ArrowUp: () => (selected === undefined ? lastInView : Math.max(0, selectedAt - 1)),
      Home: () => 0,
      End: () => last,
    };
    const move = to[event.key];
    if (!move) return;
    event.preventDefault();
    select(lines[move()]);
  };

  /** Closes the detail: Close line, or Escape inside it. The list takes keyboard focus as it goes (R10). */
  const closeDetail = () => setSelected(undefined);

  // What the list says in its place when it has no row to show (R14).
  const message = !active
    ? t("developer.noSessionEmpty")
    : !view?.read
      ? t("developer.readingProgress")
      : view.lines.length === 0
        ? t("developer.noLinesEmpty")
        : total === 0 && filtering
          ? t("developer.noMatchEmpty")
          : undefined;

  // The rows in view, and a view's height of rows either side, so a quick scroll finds rows already drawn (R08).
  const inView = measure.row > 0 ? Math.ceil(measure.height / measure.row) : 0;
  const first = measure.row > 0 ? Math.max(0, Math.floor(top / measure.row) - inView) : 0;
  const lastRow = message ? 0 : Math.min(total, first + 3 * inView + 1);
  const fresh = following ? 0 : lines.length - firstAfter(lines, seen);
  const offering = !following && total > 0 && !message;

  const rowId = (index: number) => `${ids}-row-${index}`;
  const rows = [];
  for (let row = first; row < lastRow; row++) {
    const place = { "--row": row } as CSSProperties;
    if (row < lead) {
      rows.push(
        <div key="earlier" role="option" aria-disabled="true" aria-selected={false} className="raw-log-row raw-log-earlier" style={place} aria-posinset={row + 1} aria-setsize={total}>
          {t("developer.earlierGoneDetail")}
        </div>,
      );
      continue;
    }
    const line = lines[row - lead]!;
    const from = line.dir === "backend";
    const isSelected = selected?.index === line.index;
    rows.push(
      <div
        key={line.index}
        id={rowId(line.index)}
        role="option"
        aria-selected={isSelected}
        className="raw-log-row"
        style={place}
        aria-posinset={row + 1}
        aria-setsize={total}
        data-force={force?.hover === row ? "hover" : undefined}
        onClick={() => select(line)}
      >
        <span className="raw-log-time">{rawLogTime(line.ts)}</span>
        <span className="raw-log-dir" role="img" aria-label={t(from ? "developer.fromBackendLabel" : "developer.toBackendLabel", { backend })}>
          <Icon icon={from ? ArrowDownLeft : ArrowUpRight} size="icon-xs" />
        </span>
        <span className="raw-log-text">{rowText(line)}</span>
      </div>,
    );
  }

  return (
    <div ref={root} className="raw-log">
      <div className="raw-log-area">
        {/* Always in the page, rows or not, so keyboard focus on it stays when a message takes the rows' place, as when
            the reader starts over (R18): the one stop for the keyboard (R17). */}
        <div
          ref={list}
          className="raw-log-list"
          role="listbox"
          aria-label={t("developer.listLabel")}
          aria-activedescendant={listed && selectedAt + lead >= first && selectedAt + lead < lastRow ? rowId(selected.index) : undefined}
          aria-describedby={message ? `${ids}-message` : undefined}
          tabIndex={0}
          data-force={force?.focus ? "focus" : undefined}
          onScroll={onScroll}
          onKeyDown={onKeyDown}
        >
          <div className="raw-log-rows" role="none" style={{ "--rows": message ? 0 : total } as CSSProperties}>
            {rows}
          </div>
        </div>
        {message && (
          <div className="raw-log-empty">
            <p id={`${ids}-message`}>{message}</p>
          </div>
        )}
        {/* Two lines, each coming and going by itself: the read's at the top, and the open's beneath it while both
            stand, so neither hides the other (R25). */}
        <Problem problem={view?.failure !== undefined ? { message: t("developer.unreadableError"), reason: view.failure } : undefined} />
        <Problem problem={unopened !== undefined ? { message: t("developer.openLogError"), reason: unopened } : undefined} below={view?.failure !== undefined} />
        {/* Always in the page, so it fades in and out; out of reach and unheard while hidden. */}
        <div className="raw-log-jump-place" data-shown={offering || undefined} inert={!offering} aria-hidden={!offering || undefined}>
          <button
            type="button"
            className="raw-log-jump"
            data-force={force?.jump}
            aria-label={fresh > 0 ? t("developer.jumpLabel", { count: fresh }) : t("developer.jumpButton")}
            onClick={jumpToLatest}
          >
            <Icon icon={ArrowDown} size="icon-sm" />
            <span>{t("developer.jumpButton")}</span>
            {fresh > 0 && <span className="raw-log-jump-count">{t("developer.newLinesStatus", { count: fresh })}</span>}
          </button>
        </div>
      </div>
      <RawLogDetail line={selected} backend={backend} onClose={closeDetail} force={force?.detail} />
    </div>
  );
}

/**
 * A problem, said over the top of the list so no row moves: a read that failed (R15), or the diagnostic log that
 * couldn't be opened (R13), beneath the first while both stand (R25). The message, then the reason, cut with an ellipsis, and the line whole in its tooltip. It
 * fades out with the words it had; assistive technology hears it once, from a status region that is always in the page.
 */
function Problem({ problem, below = false }: { problem: { message: string; reason: string } | undefined; below?: boolean }) {
  const [said, setSaid] = useState(problem);
  if (problem !== undefined && (problem.message !== said?.message || problem.reason !== said.reason)) setSaid(problem);
  const whole = (p: { message: string; reason: string }) => `${p.message} ${p.reason}`;
  return (
    <>
      <div className="raw-log-problem" data-shown={problem !== undefined || undefined} data-below={below || undefined} aria-hidden="true" title={said && whole(said)}>
        <span className="raw-log-problem-message">{said?.message}</span>
        <span className="raw-log-problem-reason">{said?.reason}</span>
      </div>
      <div className="heard-only" role="status">
        {problem !== undefined && whole(problem)}
      </div>
    </>
  );
}
