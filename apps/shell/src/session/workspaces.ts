// What the window knows of every project's workspaces, by id: the rail names them above their sessions, and the
// session controls read a running session's workspace from them.

import type { Project, Workspace } from "../protocol/core-api.gen";

/**
 * `known` with each project's read merged in: a read adds what it lists and never takes away an entry it did not
 * list, so a read begun before a session started in a new workspace cannot drop that workspace when it answers late.
 * A failed read (`undefined`) keeps what was known. Only the workspaces of projects no longer listed go.
 */
export function mergeWorkspaces(
  known: Readonly<Record<string, Workspace>>,
  projects: readonly Pick<Project, "id">[],
  reads: readonly (readonly Workspace[] | undefined)[],
): Record<string, Workspace> {
  const listed = new Set(projects.map((p) => p.id));
  const kept = Object.values(known).filter((w) => listed.has(w.project_id));
  return Object.fromEntries([...kept, ...reads.flatMap((r) => r ?? [])].map((w) => [w.id, w]));
}
