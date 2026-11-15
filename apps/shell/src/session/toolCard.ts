// What a tool card's row says (FLOW-READ-TRANSCRIPT-R06, R08; docs/design/TRANSCRIPT.md, Tool cards): its state,
// its detail (progress, a running clock, or its duration), and its target. Kept free of React so tests call it.

import { t } from "../catalog";
import type { Item, TargetKind } from "./transcript";

type Tool = Extract<Item, { kind: "tool" }>;

export type ToolState = "running" | "waiting" | "done" | "failed" | "stopped";

/**
 * The card's state: `waiting` when its permission request is open for the user; `stopped` once its turn or session
 * ended without its result (R16), whatever request is still open, since nothing will come of it now.
 */
export function toolState(item: Tool, waiting: boolean): ToolState {
  if (item.result) return item.result.isError ? "failed" : "done";
  if (item.stopped) return "stopped";
  return waiting ? "waiting" : "running";
}

/**
 * The row's detail at `now` (milliseconds since the epoch), or nothing:
 * - once the call has a result, its duration, from `duration_ms` when the backend gives one, else from its request's
 *   `ts` to its result's;
 * - while it runs live, its latest progress, else the time since it was requested;
 * - a call read from the record shows no clock: its progress was live only; nor does a stopped one (R16).
 * A failed or stopped call's words ("Failed", "Stopped") are the component's, not this.
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

/** A Windows folder by its spelling, from a drive letter (`D:\`, `d:/`) or a share (`\\server`): compared without case. */
const WINDOWS_FOLDER = /^([a-z]:[\\/]|\\\\)/i;

/**
 * A path as the user reads it: relative to the session's workspace folder when it lies inside it (FLOW-READ-TRANSCRIPT-R08),
 * compared with `\` and `/` alike, as the backend and Core may each write them, and, for a Windows folder, without case,
 * as Windows compares them; the rest keeps the path's own spelling, spaces included ("docs/Meeting notes.md"). Any
 * other path stays as it is.
 */
export function relativePath(path: string, workspace: string | undefined): string {
  if (!workspace) return path;
  const windows = WINDOWS_FOLDER.test(workspace);
  const norm = (p: string) => {
    const slashed = p.replace(/\\/g, "/");
    return windows ? slashed.toLowerCase() : slashed;
  };
  const root = norm(workspace).replace(/\/+$/, "");
  // Inside means past the folder and a separator: a sibling sharing its prefix ("ersa-old") is outside.
  const rest = path.slice(root.length + 1);
  return root !== "" && norm(path).startsWith(`${root}/`) && rest !== "" ? rest : path;
}

/**
 * The target as the row shows it. The event says what kind of thing the target is (PROTO-AGENT-EVENTS-R26), so a
 * command that begins with a path, or a search spelled like one, is never taken for a path: only a `path` is made
 * relative to the workspace and cut at its start. Every other kind, and a target whose event names none, keeps its
 * spelling and is cut at its end.
 */
export function shownTarget(target: string, kind: TargetKind | undefined, workspace: string | undefined): ShownTarget {
  return kind === "path" ? { text: relativePath(target, workspace), path: true } : { text: target, path: false };
}
