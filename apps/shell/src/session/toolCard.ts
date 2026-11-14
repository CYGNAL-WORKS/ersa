// What a tool card's row says (FLOW-READ-TRANSCRIPT-R06, R08; docs/design/TRANSCRIPT.md, Tool cards): its state,
// its detail (progress, a running clock, or its duration), and its target. Kept free of React so tests call it.

import { t } from "../catalog";
import type { Item } from "./transcript";

type Tool = Extract<Item, { kind: "tool" }>;

export type ToolState = "running" | "waiting" | "done" | "failed";

/** The card's state: `waiting` when its permission request is open for the user. */
export function toolState(item: Tool, waiting: boolean): ToolState {
  if (item.result) return item.result.isError ? "failed" : "done";
  return waiting ? "waiting" : "running";
}

/**
 * The row's detail at `now` (milliseconds since the epoch), or nothing:
 * - once the call has a result, its duration, from `duration_ms` when the backend gives one, else from its request's
 *   `ts` to its result's;
 * - while it runs live, its latest progress, else the time since it was requested;
 * - a call read from the record, or one the session ended without, shows no clock: its progress was live only.
 * A failed call's "Failed" is the component's, not this.
 */
export function toolDetail(item: Tool, now: number): string | undefined {
  if (item.result) {
    const ms = item.result.durationMs ?? Date.parse(item.result.at) - Date.parse(item.requestedAt);
    return formatDuration(ms);
  }
  if (!item.live || item.stopped) return undefined;
  if (item.progress) return item.progress;
  return formatDuration(now - Date.parse(item.requestedAt));
}

/**
 * A duration as CONTENT.md writes one: its two largest units, abbreviated, with no space between number and unit;
 * a unit that comes out zero after the first is left off (three minutes is "3m"). Nothing under a second, or for a time that
 * can't be read.
 */
export function formatDuration(ms: number): string | undefined {
  if (!Number.isFinite(ms) || ms < 1000) return undefined;
  const total = Math.floor(ms / 1000);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  if (hours > 0) return minutes > 0 ? t("transcript.tool.duration.hoursMinutesLabel", { hours, minutes }) : t("transcript.tool.duration.hoursLabel", { hours });
  if (minutes > 0)
    return seconds > 0 ? t("transcript.tool.duration.minutesSecondsLabel", { minutes, seconds }) : t("transcript.tool.duration.minutesLabel", { minutes });
  return t("transcript.tool.duration.secondsLabel", { seconds });
}

/** The target as the row shows it, and whether it is a path (cut at its start so the file's name stays in view). */
export interface ShownTarget {
  text: string;
  path: boolean;
}

/** An absolute path by its own spelling: from a drive letter (`D:\`, `d:/`), a share (`\\server`), or the root (`/`). */
const WINDOWS_PATH = /^([a-z]:[\\/]|\\\\)/i;
const isPath = (text: string) => WINDOWS_PATH.test(text) || (text.startsWith("/") && !/\s/.test(text));

/**
 * The target as the row shows it. The event says only what the call acts on, not what kind of thing that is, and the
 * Interface never reads a tool's input to draw a card (PROTO-AGENT-EVENTS). A target inside the session's workspace
 * folder is a path, shown relative to it (FLOW-READ-TRANSCRIPT-R08), compared with `\` and `/` alike, as the backend
 * and Core may each write them, and, for a Windows folder, without case, as Windows compares them; the rest keeps the
 * target's own spelling, spaces included ("docs/Meeting notes.md"). A command that runs a program from the workspace
 * is shown so too, as the event can't tell it from a path (docs/briefs/INBOX.md). Any other target is taken for a path
 * when it is spelled as an absolute one, so a command or a search that names a path stays as it is.
 */
export function shownTarget(target: string, workspace: string | undefined): ShownTarget {
  if (workspace) {
    const windows = WINDOWS_PATH.test(workspace);
    const norm = (p: string) => {
      const slashed = p.replace(/\\/g, "/");
      return windows ? slashed.toLowerCase() : slashed;
    };
    const root = norm(workspace).replace(/\/+$/, "");
    // Inside means past the folder and a separator: a sibling sharing its prefix ("ersa-old") is outside.
    const rest = target.slice(root.length + 1);
    if (root !== "" && norm(target).startsWith(`${root}/`) && rest !== "") return { text: rest, path: true };
  }
  return { text: target, path: isPath(target) };
}
