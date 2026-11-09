// The rail's tree, decided from Core's projects, workspaces and sessions and the rail's arrangement (LAY-SHELL, The
// rail; FLOW-ORGANIZE-SESSIONS): the pinned sessions, the groups with headings, and the sessions under none. The
// component only draws this.

import type { Project, Session, Workspace } from "../protocol/core-api.gen";
import { newestFirst, timeOf } from "./controls";
import { DEFAULT_ARRANGEMENT, type GroupBy, type Show, type SortBy } from "./arrangement";
import { wantTone } from "./tile";

/** A date group's key, also its key among the collapsed groups (PROTO-CORE-API, Settings). */
export type DateKey = "today" | "yesterday" | "previous_7_days" | "older";
export const DATE_KEYS: readonly DateKey[] = ["today", "yesterday", "previous_7_days", "older"];

/** A group with a heading, after the Pinned group: a project, or a date group. Its `key` is its collapsed-group key. */
export type Group =
  | { kind: "project"; key: string; project: Project; sessions: Session[] }
  | { kind: "date"; key: DateKey; sessions: Session[] };

export interface Tree {
  /** The pinned sessions shown, in the chosen sort: they lead the rail in the Pinned group and appear nowhere else (FLOW-ORGANIZE-SESSIONS-R01). */
  pinned: Session[];
  /** The groups with headings, in order, each with its sessions in the chosen sort. Grouped by project, a project may have none. */
  groups: Group[];
  /**
   * The sessions under no heading, in the chosen sort: grouped by project, those whose project the window can't tell, at
   * the end; grouped by none, every session not pinned (R08).
   */
  loose: Session[];
}

/** How the tree is arranged: the grouping, the sort and the filter of the rail's arrangement, and today, for date groups. */
export interface TreeArrangement {
  groupBy: GroupBy;
  sortBy: SortBy;
  show: Show;
  /** Now, whose local date the date groups count from. */
  now: Date;
}

/**
 * The tree. The filter leaves out what it does not show, pinned sessions too (R11). A pinned session is in the
 * Pinned group, wherever it belongs. Grouped by project, any other belongs to the project of its workspace; one whose
 * workspace is not known, or whose workspace's project is not listed, is loose, so a session is never missing from
 * the rail. While showing active sessions every project has its group, empty or not; otherwise only projects with
 * sessions shown (LAY-SHELL, Projects). Grouped by date, each is in its date group (R07), and only groups with
 * sessions are there. Pinning changes none of this: unpinned, a session is back where it belongs.
 */
export function treeOf(
  projects: readonly Project[],
  workspaces: Readonly<Record<string, Pick<Workspace, "project_id">>>,
  sessions: readonly Session[],
  { groupBy, sortBy, show, now }: TreeArrangement = { ...DEFAULT_ARRANGEMENT, now: new Date() },
): Tree {
  const pinned: Session[] = [];
  const loose: Session[] = [];
  const byProject = new Map<string, Session[]>(projects.map((p) => [p.id, []]));
  const byDate = new Map<DateKey, Session[]>(DATE_KEYS.map((k) => [k, []]));
  for (const s of sortedBy(sessions, sortBy)) {
    if (!shows(show, s)) continue;
    if (s.pinned) {
      pinned.push(s);
      continue;
    }
    if (groupBy === "date") {
      byDate.get(dateGroupOf(s.created_at, now))?.push(s);
      continue;
    }
    const projectId = groupBy === "project" ? workspaces[s.workspace_id]?.project_id : undefined;
    const group = projectId === undefined ? undefined : byProject.get(projectId);
    if (group) group.push(s);
    else loose.push(s);
  }
  const groups: Group[] =
    groupBy === "project"
      ? projects
          .map((project) => ({ kind: "project" as const, key: project.id, project, sessions: byProject.get(project.id) ?? [] }))
          .filter((g) => show === "active" || g.sessions.length > 0)
      : groupBy === "date"
        ? DATE_KEYS.map((key) => ({ kind: "date" as const, key, sessions: byDate.get(key) ?? [] })).filter((g) => g.sessions.length > 0)
        : [];
  return { pinned, groups, loose };
}

// The user's locale, as the webview reports it (R09).
const names = new Intl.Collator();

/** A session's last activity, for sorting: its creation until a turn has ended (R09). */
function lastActivityOf(session: Session): string {
  return session.last_activity_at ?? session.created_at;
}

function byName(a: Session, b: Session): number {
  // Untitled sessions go last, and tie with each other.
  if (!a.title || !b.title) return (a.title ? 0 : 1) - (b.title ? 0 : 1);
  return names.compare(a.title, b.title);
}

/** Each sort: what it reads of a session, and how it compares two; a tie keeps the order it is given (R09). */
const SORTS: Record<SortBy, { read: (s: Session) => string | undefined; compare: (a: Session, b: Session) => number }> = {
  created: { read: (s) => s.created_at, compare: () => 0 },
  last_activity: { read: lastActivityOf, compare: (a, b) => timeOf(lastActivityOf(b)) - timeOf(lastActivityOf(a)) },
  name: { read: (s) => s.title, compare: byName },
};

/** The sessions in the chosen sort, ties by date created, newest first (R09), since a sort keeps the order of equals. */
function sortedBy(sessions: readonly Session[], sortBy: SortBy): Session[] {
  return newestFirst(sessions).sort(SORTS[sortBy].compare);
}

/** What the chosen sort reads of a session, its tie-break included: while it holds, the tile keeps its place (R10). */
export function sortValueOf(session: Session, sortBy: SortBy): string {
  return `${SORTS[sortBy].read(session)}/${session.created_at}`;
}

/** Whether the filter shows the session (R11): showing All, an archived one too, its tile marked as archived. */
function shows(show: Show, session: Session): boolean {
  // A session from an older Core may carry no archived mark: it is not archived.
  return show === "all" || (show === "archived") === (session.archived === true);
}

const DAY = 24 * 60 * 60 * 1000;

/**
 * The date group of a session created at `createdAt`, by its local date against today's (R07): Today, Yesterday,
 * Previous 7 days for the six days before those, else Older. By creation, so a session never changes group as it
 * works; one created later than now, by a clock set wrong, is Today.
 */
export function dateGroupOf(createdAt: string, now: Date): DateKey {
  const created = new Date(createdAt);
  // Calendar days apart in local time; rounded, since a day across a daylight-saving change is 23 or 25 hours.
  const days = Math.round((localMidnight(now) - localMidnight(created)) / DAY);
  if (Number.isNaN(days)) return "older";
  if (days <= 0) return "today";
  if (days === 1) return "yesterday";
  return days <= 7 ? "previous_7_days" : "older";
}

function localMidnight(at: Date): number {
  return new Date(at.getFullYear(), at.getMonth(), at.getDate()).getTime();
}

/** What a collapsed group heading says of the sessions it holds (LAY-SHELL, Collapsing). */
export interface Collapsed {
  count: number;
  /** When any of them needs the user, the tone of what they want: a permission request's over a question's. */
  tone?: "accent" | "caution";
}

/** What a collapsed heading says: how many sessions it holds, and the tone of the mark that leads the count. */
export function collapsedOf(sessions: readonly Session[]): Collapsed {
  const tones = sessions.filter((s) => s.tile.pending.length > 0).map((s) => wantTone(s.tile.pending));
  const tone = tones.includes("caution") ? "caution" : tones.length > 0 ? "accent" : undefined;
  return { count: sessions.length, tone };
}
