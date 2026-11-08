// The rail's tree, decided from Core's projects, workspaces and sessions (LAY-SHELL, The rail): each project with
// its sessions beneath it, and the sessions whose project the window can't tell. The component only draws this.

import type { Project, Session, Workspace } from "../protocol/core-api.gen";
import { newestFirst } from "./controls";
import { wantTone } from "./tile";

export interface Tree {
  /** Every project, in the order Core lists them, each with its sessions newest first; a project may have none. */
  projects: { project: Project; sessions: Session[] }[];
  /** The sessions whose project the window can't tell, newest first: they sit at the end, under no heading. */
  loose: Session[];
}

/**
 * The tree. A session belongs to the project of its workspace; a session whose workspace is not known, or whose
 * workspace's project is not listed, is loose, so a session is never missing from the rail. Archived sessions are
 * left out: the rail shows active sessions, its default (FLOW-ARCHIVE-DELETE-SESSION, step 1), until the arrange
 * menu's Show can ask for archived ones.
 */
export function treeOf(
  projects: readonly Project[],
  workspaces: Readonly<Record<string, Pick<Workspace, "project_id">>>,
  sessions: readonly Session[],
): Tree {
  const byProject = new Map<string, Session[]>(projects.map((p) => [p.id, []]));
  const loose: Session[] = [];
  for (const s of newestFirst(sessions)) {
    if (s.archived) continue;
    const projectId = workspaces[s.workspace_id]?.project_id;
    const group = projectId === undefined ? undefined : byProject.get(projectId);
    if (group) group.push(s);
    else loose.push(s);
  }
  return { projects: projects.map((project) => ({ project, sessions: byProject.get(project.id) ?? [] })), loose };
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
