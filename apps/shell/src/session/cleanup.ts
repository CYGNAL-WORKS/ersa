// The Clean up list in Storage (FLOW-CLEAN-UP, steps 5 to 7; R16 to R19, R28, R29): what each item loses, which
// are ticked to begin with, what the question says, and what `cleanup.remove` is sent. Free of React, so every
// decision is made, and tested, without drawing anything; the hook in `cleanupList.ts` runs the round trips.

import { t } from "../catalog";
import type { CleanupInspectResult, CleanupItem, CleanupKind, Session } from "../protocol/core-api.gen";
import { titleOf } from "./tile";

/** How long without activity makes a session inactive: Claude Desktop's choices (`claude-desktop/cleanup-2026-11-20.txt`). */
export const CLEAN_UP_DAYS = [30, 60, 90] as const;
export type CleanUpDays = (typeof CLEAN_UP_DAYS)[number];

/** The groups, in the order the brief lists them (FLOW-CLEAN-UP, step 5). */
export const GROUPS: readonly CleanupKind[] = ["workspace", "leftover", "branch", "folder"];

/** An item on the list: still being checked, checked, or one Core couldn't check and why. */
export type ItemView =
  | { state: "checking"; item: CleanupItem }
  | { state: "inspected"; item: CleanupItem; inspected: CleanupInspectResult }
  | { state: "uninspectable"; item: CleanupItem; message: string };

const counted = (n: number | undefined) => (n ?? 0) > 0;

/**
 * Whether `cleanup.remove` must be sent `discard` for the item, as Core judges it (PROTO-CORE-API, Clean up;
 * FLOW-CLEAN-UP-R13, R27): uncommitted files, commits only the folder holds, commits git can't count or a repository
 * of its own for a workspace or a leftover; commits no other branch holds for a branch, merged or not, since they go
 * with it; and every folder. A leftover's branch holding commits of its own is kept without `discard`, so they are
 * not sent as lost and its branch stays. Ignored files are no unsaved work.
 */
export function sendsDiscard(inspected: CleanupInspectResult): boolean {
  switch (inspected.kind) {
    case "workspace":
    case "leftover":
      return counted(inspected.uncommitted) || counted(inspected.stranded) || inspected.unrecorded === true || inspected.holds_repository === true;
    case "branch":
      return counted(inspected.unshared);
    case "folder":
      return true;
  }
}

/**
 * Whether removing the item loses unsaved work, as the list and the question say it (R16, R28): what Core sends with
 * `discard`, except a branch whose changes the base branch already holds (`merged`), which loses nothing a person
 * would miss. A folder Ersa can't identify is not counted as losing work, though it goes only with `discard`: Core
 * counts nothing in it, and the list says so.
 */
export function losesWork(inspected: CleanupInspectResult): boolean {
  if (inspected.kind === "folder") return false;
  if (inspected.kind === "branch" && inspected.merged === true) return false;
  return sendsDiscard(inspected);
}

/**
 * Whether the item is ticked before the user ticks or unticks it (R16): exactly those whose removal loses no unsaved
 * work, a merged branch among them, and never a folder. One still being checked, or that couldn't be, is unticked.
 */
export function tickedByDefault(view: ItemView): boolean {
  return view.state === "inspected" && view.inspected.kind !== "folder" && !losesWork(view.inspected);
}

/** A folder Ersa couldn't finish looking through: Core reports it without a size (R29). */
export const unfinished = (inspected: CleanupInspectResult) => inspected.kind === "folder" && inspected.size_bytes === undefined;

/**
 * What the item is called on the list: its sessions' titles for a workspace, its branch for a branch, else its
 * folder's own name, the list showing where it is beneath. A session the window doesn't hold reads as untitled.
 */
export function itemName(item: CleanupItem, sessions: readonly Pick<Session, "id" | "title">[]): string {
  if (item.kind === "workspace" && item.session_ids && item.session_ids.length > 0) {
    return item.session_ids.map((id) => titleOf(sessions.find((s) => s.id === id) ?? { title: "" })).join(", ");
  }
  if (item.kind === "branch") return item.branch ?? "";
  return folderName(item.path) ?? item.branch ?? "";
}

/** The last part of a folder's path, either separator: "D:/wt/alpha/stray" is "stray". */
export function folderName(path: string | undefined): string | undefined {
  if (path === undefined) return undefined;
  const parts = path.split(/[\\/]/).filter((p) => p !== "");
  return parts.at(-1) ?? path;
}

/** What removing the item loses, counted: changed files and commits that exist nowhere else. */
export function lostCounts(inspected: CleanupInspectResult): { files: number; commits: number } {
  if (inspected.kind === "workspace" || inspected.kind === "leftover") {
    return { files: inspected.uncommitted ?? 0, commits: inspected.stranded ?? 0 };
  }
  if (inspected.kind === "branch" && inspected.merged !== true) return { files: 0, commits: inspected.unshared ?? 0 };
  return { files: 0, commits: 0 };
}

// The counts read as one list: "3 changed files and 2 commits that exist nowhere else".
const list = new Intl.ListFormat("en-US", { type: "conjunction" });

/** The counts as words, or undefined when there are none. */
export function lostParts({ files, commits }: { files: number; commits: number }): string | undefined {
  const parts: string[] = [];
  if (files > 0) parts.push(String(t("settings.storage.cleanUp.changedFilesPart", { count: files })));
  if (commits > 0) parts.push(String(t("settings.storage.cleanUp.commitsPart", { count: commits })));
  return parts.length > 0 ? list.format(parts) : undefined;
}

/** What the question says of the ticked items (R17, R28, R29). */
export interface Question {
  /** Every item but a branch removes a folder, or puts one away. */
  folders: number;
  branches: number;
  /** The space freed, leaving out what wasn't measured. */
  bytes: number;
  /** Whether any ticked item has a folder, so a space freed is said. */
  freesSpace: boolean;
  /** Whether any ticked item is a session's folder, which keeps its conversation and branch. */
  sessions: boolean;
  files: number;
  commits: number;
  /** How many folders hold commits git can't count. */
  unrecorded: number;
  /** How many folders hold a repository of their own (R28). */
  repositories: number;
  /** How many folders Ersa couldn't finish looking through, left out of `bytes` (R29). */
  unfinished: number;
  /** How many measured folders Ersa can't identify, whose contents it can't count. */
  strays: number;
  /** Whether the confirming button reads as discarding: something ticked loses work, or is a folder (step 6). */
  discards: boolean;
}

export function questionOf(ticked: readonly CleanupInspectResult[]): Question {
  const q: Question = { folders: 0, branches: 0, bytes: 0, freesSpace: false, sessions: false, files: 0, commits: 0, unrecorded: 0, repositories: 0, unfinished: 0, strays: 0, discards: false };
  for (const i of ticked) {
    if (i.kind === "branch") q.branches += 1;
    else {
      q.folders += 1;
      q.freesSpace = true;
    }
    if (i.kind === "workspace") q.sessions = true;
    q.bytes += i.size_bytes ?? 0;
    const lost = lostCounts(i);
    q.files += lost.files;
    q.commits += lost.commits;
    if (i.unrecorded === true) q.unrecorded += 1;
    if (i.holds_repository === true) q.repositories += 1;
    if (unfinished(i)) q.unfinished += 1;
    else if (i.kind === "folder") q.strays += 1;
    if (losesWork(i) || i.kind === "folder") q.discards = true;
  }
  return q;
}

/** The question's title: "Clean up 3 folders and 2 branches?". */
export function questionTitle(q: Pick<Question, "folders" | "branches">): string {
  const parts: string[] = [];
  if (q.folders > 0) parts.push(String(t("settings.storage.cleanUp.foldersPart", { count: q.folders })));
  if (q.branches > 0) parts.push(String(t("settings.storage.cleanUp.branchesPart", { count: q.branches })));
  return String(t("settings.storage.cleanUp.questionTitle", { parts: list.format(parts) }));
}

const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/**
 * A size as File Explorer words it, in units of 1,024 named KB, MB and GB: Ersa is Windows first, and Explorer is
 * where people see how big a folder is. One decimal below 10 and none from 10: "1.2 GB", "48 MB", "0 B".
 */
export function formatSize(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const shown = unit === 0 || value >= 10 ? Math.round(value).toString() : (Math.round(value * 10) / 10).toString();
  return `${shown} ${UNITS[unit]}`;
}

/**
 * The item as it is now, when Core refused its removal because it changed since it was checked: `invalid_state` whose
 * `detail` is the item as `cleanup.inspect` would now give it, as JSON text (PROTO-CORE-API, Clean up). Undefined for
 * any other refusal, whose detail is never that.
 */
export function changedTo(detail: string | undefined, id: string): CleanupInspectResult | undefined {
  if (detail === undefined) return undefined;
  try {
    const now: unknown = JSON.parse(detail);
    if (typeof now !== "object" || now === null) return undefined;
    const item = now as Partial<CleanupInspectResult>;
    return item.id === id && typeof item.token === "string" && typeof item.kind === "string" ? (item as CleanupInspectResult) : undefined;
  } catch {
    return undefined;
  }
}
