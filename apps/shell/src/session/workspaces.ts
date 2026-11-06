// What the window knows of every project's workspaces, by id: the rail puts a session under its workspace's project,
// and the session controls read a running session's workspace from them.

import type { Project, Workspace } from "../protocol/core-api.gen";

/**
 * `known` with one project's read merged in: a read adds what it lists and never takes away an entry it did not
 * list, so a read begun before a session started in a new workspace cannot drop that workspace when it answers late.
 * A failed read (`undefined`) keeps what was known. Only the workspaces of projects no longer listed go.
 */
export function mergeWorkspaces(
  known: Readonly<Record<string, Workspace>>,
  projects: readonly Pick<Project, "id">[],
  read: readonly Workspace[] | undefined,
): Record<string, Workspace> {
  const listed = new Set(projects.map((p) => p.id));
  const kept = Object.values(known).filter((w) => listed.has(w.project_id));
  return Object.fromEntries([...kept, ...(read ?? [])].map((w) => [w.id, w]));
}
