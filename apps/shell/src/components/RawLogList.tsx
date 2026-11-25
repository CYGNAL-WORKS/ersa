import { ArrowDown, ArrowDownLeft, ArrowUpRight } from "lucide-react";
import { type CSSProperties, type UIEvent, useCallback, useLayoutEffect, useRef, useState } from "react";
import { t } from "../catalog";
import type { RawLogEntry } from "../protocol/core-api.gen";
import type { RawLogView } from "../session/rawLog";
import { Icon } from "./Icon";
import "./RawLogList.css";

/** A row draws at most this many characters of its line (docs/design/WORKBENCH.md, The list). */
export const ROW_CHARACTERS = 2_000;

/** `ts` as 24-hour `HH:MM:SS.mmm` in the user's local time (FLOW-READ-RAW-LOG-R03); as sent when it isn't a time. */
export function rawLogTime(ts: string): string {
  const at = new Date(ts);
  if (Number.isNaN(at.getTime())) return ts;
  const two = (n: number) => String(n).padStart(2, "0");
  return `${two(at.getHours())}:${two(at.getMinutes())}:${two(at.getSeconds())}.${String(at.getMilliseconds()).padStart(3, "0")}`;
}

/** The start a row draws: the line, or the start Core sent of a cut one, to at most ROW_CHARACTERS characters. */
export function rowText(entry: RawLogEntry): string {
  const text = "line" in entry ? entry.line : entry.head;
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
 * The developer pane's list of the active session's raw log (FLOW-READ-RAW-LOG, steps 2 and 3; docs/design/WORKBENCH.md,
 * The list): a row a line, oldest first, following the end as lines arrive until the user scrolls away; then the rows
 * in view stay put and Jump to latest, counting the lines that arrived since, takes them back. Only the rows in or
 * near view are in the page (R08). With nothing to list it says why (R14), and a read that failed lies over the top of
 * the list as one line (R15). Keyed by session where it is used, so a new session starts at its end, following.
 */
export function RawLogList({
  view: given,
  active,
  backend,
  force,
}: {
  /** What the reader holds of the active session's raw log; undefined before it has read anything. */
  view: RawLogView | undefined;
  /** Whether there is an active session: none while the new-session screen shows. */
  active: boolean;
  /** The session's backend, by its own label, for the direction assistive technology hears. */
  backend: string;
  /** For the gallery: scrolled away with `away` new lines, Jump to latest in a pointer or keyboard state, a row under the pointer. */
  force?: { away?: number; jump?: "hover" | "pressed" | "focus"; hover?: number };
}) {
  const list = useRef<HTMLDivElement>(null);
  // The reader reports nothing while the workbench closes, and the list runs back with what it showed, not a message.
  const [kept, setKept] = useState(given);
  if (given !== undefined && given !== kept) setKept(given);
  const view = given ?? (active ? kept : undefined);
  const lines = view?.lines ?? [];
  const earlier = view?.earlier ?? false;
  // The earlier-lines row, when there is one, is the list's first row; line n is then row n + 1.
  const lead = earlier ? 1 : 0;
  const total = lead + lines.length;

  const [measure, setMeasure] = useState<Measure>({ row: 0, height: 0 });
  const [top, setTop] = useState(0);
  // Following the end, as it does until the user scrolls away from it.
  const [following, setFollowing] = useState(force?.away === undefined);
  // The last index held when the user left the end; lines above it arrived since.
  const [seen, setSeen] = useState(() => (force?.away === undefined ? -1 : (lines.at(-1)?.index ?? 0) - force.away));
  const anchor = useRef<Anchor>({ index: -1, offset: 0 });

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

  // The row token and the list's height, read where the list is drawn, and again whenever its size changes.
  useLayoutEffect(() => {
    const el = list.current;
    if (!el) return;
    const read = () => {
      const row = parseFloat(getComputedStyle(el).getPropertyValue("--size-log-row"));
      setMeasure((was) => (was.row === row && was.height === el.clientHeight ? was : { row, height: el.clientHeight }));
    };
    read();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(read);
    observer.observe(el);
    return () => observer.disconnect();
  }, [active, total > 0]);

  // After each change of what the list holds or of its size: at the end while following, new rows appearing at once
  // (WORKBENCH.md, New rows); else the line the user reads stays where it was in the view.
  useLayoutEffect(() => {
    const el = list.current;
    if (!el || measure.row <= 0) return;
    const end = Math.max(0, total * measure.row - measure.height);
    let next: number;
    if (following) next = end;
    else {
      const { index, offset } = anchor.current;
      // The earlier-lines row is row 0; a line gone from the front leaves the view at the top.
      const at = index < 0 ? 0 : lead + firstAfter(lines, index - 1);
      next = index >= 0 && lines[at - lead]?.index === index ? Math.min(end, at * measure.row + offset) : index < 0 ? Math.min(end, offset) : 0;
    }
    if (el.scrollTop !== next) el.scrollTop = next;
    anchor.current = anchorAt(next);
    setTop(next);
  }, [lines, lead, total, measure, following, anchorAt]);

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
  // the list, never to the window.
  const jumpToLatest = (event: { currentTarget: HTMLElement }) => {
    if (event.currentTarget === document.activeElement) list.current?.focus({ preventScroll: true });
    setFollowing(true);
  };

  if (!active) return <Empty text={t("developer.noSessionEmpty")} problem={undefined} />;
  if (!view?.read) return <Empty text={t("developer.readingProgress")} problem={view?.failure} />;
  if (total === 0) return <Empty text={t("developer.noLinesEmpty")} problem={view.failure} />;

  // The rows in view, and a view's height of rows either side, so a quick scroll finds rows already drawn (R08).
  const inView = measure.row > 0 ? Math.ceil(measure.height / measure.row) : 0;
  const first = measure.row > 0 ? Math.max(0, Math.floor(top / measure.row) - inView) : 0;
  const last = Math.min(total, first + 3 * inView + 1);
  const fresh = following ? 0 : lines.length - firstAfter(lines, seen);
  const offering = !following;

  const rows = [];
  for (let row = first; row < last; row++) {
    const place = { "--row": row } as CSSProperties;
    if (row < lead) {
      rows.push(
        <div key="earlier" role="listitem" className="raw-log-row raw-log-earlier" style={place} aria-posinset={row + 1} aria-setsize={total}>
          {t("developer.earlierGoneDetail")}
        </div>,
      );
      continue;
    }
    const line = lines[row - lead]!;
    const from = line.dir === "backend";
    rows.push(
      <div
        key={line.index}
        role="listitem"
        className="raw-log-row"
        style={place}
        aria-posinset={row + 1}
        aria-setsize={total}
        data-force={force?.hover === row ? "hover" : undefined}
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
    <div className="raw-log">
      <div ref={list} className="raw-log-list" role="list" aria-label={t("developer.listLabel")} tabIndex={0} onScroll={onScroll}>
        <div className="raw-log-rows" role="none" style={{ "--rows": total } as CSSProperties}>
          {rows}
        </div>
      </div>
      <Problem failure={view.failure} />
      {/* Always in the page while there are rows, so it fades in and out; out of reach and unheard while hidden. */}
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
  );
}

/** What the list says in its place when it has nothing to list (R14), under the problem line when a read failed. */
function Empty({ text, problem }: { text: string; problem: string | undefined }) {
  return (
    <div className="raw-log">
      <p className="raw-log-empty">{text}</p>
      <Problem failure={problem} />
    </div>
  );
}

/**
 * A read that failed, said over the top of the list so no row moves (R15): the message, then the reason, cut with an
 * ellipsis, and the line whole in its tooltip. It fades out with the words it had; assistive technology hears it once, from a
 * status region that is always in the page.
 */
function Problem({ failure }: { failure: string | undefined }) {
  const [said, setSaid] = useState(failure);
  if (failure !== undefined && failure !== said) setSaid(failure);
  const message = t("developer.unreadableError");
  return (
    <>
      <div className="raw-log-problem" data-shown={failure !== undefined || undefined} aria-hidden="true" title={said && `${message} ${said}`}>
        <span className="raw-log-problem-message">{message}</span>
        <span className="raw-log-problem-reason">
          {said}
        </span>
      </div>
      <div className="heard-only" role="status">
        {failure !== undefined && `${message} ${failure}`}
      </div>
    </>
  );
}
