// Reading a session's raw log for the developer pane (FLOW-READ-RAW-LOG, steps 2, 3 and 7): the lines Ersa and the
// session's backend exchanged, as Core holds them (PROTO-CORE-API, `session.raw_log`). Core sends no notification for
// them, so while the pane is open the window asks once a second for the lines after the last it holds. Kept free of
// React, like core/connection.ts, so the tests drive it with a fake client and a fake clock.

import { t } from "../catalog";
import { type CoreClient, describeError } from "../core/client";
import type { RawLogEntry, SessionRawLogParams } from "../protocol/core-api.gen";

/** How often to ask while the pane is open (FLOW-READ-RAW-LOG-R05). */
export const RAW_LOG_EVERY_MS = 1_000;
/** The most entries one answer may hold (PROTO-CORE-API, `session.raw_log`); an answer this full means more are waiting. */
export const RAW_LOG_PAGE = 1000;
/** The most lines the pane holds for a session; the oldest go first (FLOW-READ-RAW-LOG-R09). */
export const RAW_LOG_HELD = 50_000;
/** Bytes: a line longer than this comes back as its start and size (FLOW-READ-RAW-LOG-R11; PROTO-CORE-API-R138). */
export const RAW_LOG_CLIP = 262_144;

/** What the pane has of the active session's raw log. */
export interface RawLogView {
  session: string;
  /** Oldest first, at most {@link RAW_LOG_HELD}. A line longer than the clip carries `head` and `size` in place of `line`. */
  lines: RawLogEntry[];
  /** Whether an answer for this session has come yet; until then the pane says it is reading (FLOW-READ-RAW-LOG-R14). */
  read: boolean;
  /** Lines before the first listed are no longer held, by the pane or by Core (R09). */
  earlier: boolean;
  /** Why the last read failed; absent once one succeeds (R15). */
  failure?: string;
}

/** What the window tells the reader: whether the workbench is open, the active session, and the connection's number. */
export interface RawLogWanted {
  open: boolean;
  session: string | undefined;
  /**
   * `CoreView.link`: a new number means Ersa connected to Core again (R18). Undefined while the connection is lost,
   * which changes nothing: reads fail meanwhile, and the pane keeps its lines (R15).
   */
  link: number | undefined;
}

export interface RawLogReader {
  /** Called whenever the workbench opens or closes, the active session changes or Ersa connects again. */
  show(wanted: RawLogWanted): void;
  /** After this, the reader asks for nothing and reports nothing. */
  stop(): void;
}

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const isCount = (v: unknown): v is number => Number.isInteger(v) && (v as number) >= 0;

/** Whether `value` is one entry as the protocol gives it: whole, or cut to its head and size. */
function isEntry(value: unknown): value is RawLogEntry {
  if (!isObject(value) || !isCount(value.index) || typeof value.ts !== "string") return false;
  if (value.dir !== "backend" && value.dir !== "host") return false;
  return typeof value.line === "string" || (typeof value.head === "string" && isCount(value.size));
}

/**
 * The entries of an answer to a read from `from`, or undefined when it can't be read: Core sends them oldest first,
 * each index above the last and none before `from` (PROTO-CORE-API-R136). A Core that breaks that is a failed read,
 * not a reason to lose the lines held.
 */
function entriesOf(answer: unknown, from: number): RawLogEntry[] | undefined {
  if (!Array.isArray(answer) || !answer.every(isEntry)) return undefined;
  let last = from - 1;
  for (const entry of answer) {
    if (entry.index <= last) return undefined;
    last = entry.index;
  }
  return answer;
}

/**
 * Reads the raw log of the session `show` names while the workbench is open, and reports what it holds through
 * `update`: a view, or undefined while it reads nothing (closed, or no active session).
 */
export function readRawLog(client: CoreClient, update: (view: RawLogView | undefined) => void): RawLogReader {
  let wanted: RawLogWanted = { open: false, session: undefined, link: undefined };
  let view: RawLogView | undefined;
  let stopped = false;
  // Each start of reading gets a number; an answer to a read sent before the latest start is dropped, so a late answer
  // never brings back lines of a session the pane left, or of a Core it connected to before (R04, R18).
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const report = (next: RawLogView | undefined) => {
    view = next;
    update(next);
  };

  /** One read, from the line after the last held; then the next, a second after this one was sent, or at once when it was full. */
  const ask = (mine: number, session: string) => {
    timer = undefined;
    const held = view?.lines ?? [];
    const last = held.at(-1);
    const from = last === undefined ? 0 : last.index + 1;
    // With nothing held, no `from`: Core starts at the first line it holds.
    const params: SessionRawLogParams = last === undefined ? { id: session, clip: RAW_LOG_CLIP } : { id: session, from, clip: RAW_LOG_CLIP };
    const sent = Date.now();
    // Only one read is out at a time: the next is scheduled when this one settles.
    const next = (full: boolean) => {
      if (stopped || mine !== generation) return;
      const wait = full ? 0 : Math.max(0, sent + RAW_LOG_EVERY_MS - Date.now());
      timer = setTimeout(() => ask(mine, session), wait);
    };
    client.call("session.raw_log", params).then(
      (answer) => {
        if (stopped || mine !== generation || view === undefined) return;
        const entries = entriesOf(answer, from);
        if (entries === undefined) {
          report({ ...view, read: true, failure: t("developer.unreadableReason") });
          next(false);
          return;
        }
        // An answer that starts later than asked: Core let the lines between go, so the pane lets go of what it held
        // and lists from that answer's first line (R09).
        const first = entries[0];
        const kept = first !== undefined && first.index > from ? [] : view.lines;
        const lines = kept.concat(entries);
        const trimmed = lines.length > RAW_LOG_HELD ? lines.slice(lines.length - RAW_LOG_HELD) : lines;
        // Core numbers a session's lines from 0, so a first line above 0 means earlier ones are gone (PROTO-CORE-API-R136).
        const earlier = (trimmed[0]?.index ?? 0) > 0;
        report({ session, lines: trimmed, read: true, earlier });
        next(entries.length >= RAW_LOG_PAGE);
      },
      (error: unknown) => {
        if (stopped || mine !== generation || view === undefined) return;
        // Keep the lines held and go on asking (R15).
        report({ ...view, read: true, failure: describeError(error) });
        next(false);
      },
    );
  };

  /** Lets go of whatever is held and of any read out; then reads `wanted`'s session from the start, if the pane is open. */
  const restart = () => {
    generation++;
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    const { open, session } = wanted;
    if (!open || session === undefined) {
      if (view !== undefined) report(undefined);
      return;
    }
    report({ session, lines: [], read: false, earlier: false });
    ask(generation, session);
  };

  return {
    show(next) {
      if (stopped) return;
      const before = wanted;
      // A lost connection keeps the last connection's number, so only a new connection starts reading again.
      wanted = next.link === undefined ? { ...next, link: before.link } : next;
      // Reading starts again from the first line Core holds when the pane opens or closes, the active session changes
      // (R04), or Ersa connects to Core again (R18): a Core that restarted numbers its lines from the start.
      if (before.open !== wanted.open || before.session !== wanted.session || before.link !== wanted.link) restart();
    },
    stop() {
      stopped = true;
      generation++;
      if (timer !== undefined) clearTimeout(timer);
      timer = undefined;
    },
  };
}
