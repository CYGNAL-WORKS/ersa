// The rail's tree, decided from Core's projects, workspaces and sessions (LAY-SHELL, The rail): each project with
// its sessions beneath it, and the sessions whose project the window can't tell. The component only draws this.

import type { Project, Session, Workspace } from "../protocol/core-api.gen";
import { newestFirst } from "./controls";

export interface Tree {
  /** Every project, in the order Core lists them, each with its sessions newest first; a project may have none. */
  projects: { project: Project; sessions: Session[] }[];
  /** The sessions whose project the window can't tell, newest first: they sit at the end, under no heading. */
  loose: Session[];
}

/**
 * The tree. A session belongs to the project of its workspace; a session whose workspace is not known, or whose
 * workspace's project is not listed, is loose, so a session is never missing from the rail.
 */
export function treeOf(
  projects: readonly Project[],
  workspaces: Readonly<Record<string, Pick<Workspace, "project_id">>>,
  sessions: readonly Session[],
): Tree {
  const byProject = new Map<string, Session[]>(projects.map((p) => [p.id, []]));
  const loose: Session[] = [];
  for (const s of newestFirst(sessions)) {
    const projectId = workspaces[s.workspace_id]?.project_id;
    const group = projectId === undefined ? undefined : byProject.get(projectId);
    if (group) group.push(s);
    else loose.push(s);
  }
  return { projects: projects.map((project) => ({ project, sessions: byProject.get(project.id) ?? [] })), loose };
}
