// The session controls' values: what a new session will start with (FLOW-NEW-SESSION, LAY-SHELL): host,
// project, workspace, backend, model, effort and permission mode. The choices come from Core's `project.list`
// and `backend.list`; the starting values from what the user last chose for the project, else the brief's defaults.

import type { Project } from "../protocol/core-api.gen";

/** One choice a backend offers: a model, an effort or a permission mode (PROTO-CORE-API, `backend.list`). */
export interface Choice {
  id: string;
  /** What the interface shows. */
  label: string;
  /** The one a new session starts with. */
  default: boolean;
}

/**
 * One entry of `backend.list { host_id }` (PROTO-CORE-API, Backends and modes). Typed by hand until Core serves
 * it and the generated types carry it; the shape is the brief's.
 */
export interface BackendChoices {
  name: string;
  /** Present when available. */
  version?: string;
  status: "available" | "missing" | "error";
  tested: { min: string; max: string };
  models: Choice[];
  efforts: Choice[];
  permission_modes: Choice[];
}

/** A new isolated workspace (a git worktree for a git project), or the project's own folder. */
export type WorkspaceKind = "isolated" | "root";

/** A session's values: what it starts with, as the session controls show them. The backend's choices are absent while the host has no backend to run. */
export interface SessionValues {
  host_id: string;
  project_id: string;
  workspace: WorkspaceKind;
  backend?: string;
  model?: string;
  effort?: string;
  permission_mode?: string;
}

/** The version control Ersa can make isolated workspaces for. Others run in the root workspace only. */
const WORKSPACE_ADAPTERS: readonly Project["vcs"][] = ["git"];

/** The workspaces a project offers: a new isolated one first where Ersa has an adapter, and always the root. */
export function workspaceKinds(project: Project): WorkspaceKind[] {
  return WORKSPACE_ADAPTERS.includes(project.vcs) ? ["isolated", "root"] : ["root"];
}

/** The backends a session can start with: those detected as available. */
export function available(backends: readonly BackendChoices[]): BackendChoices[] {
  return backends.filter((b) => b.status === "available");
}

/**
 * Whether the backend chip shows: when the host has more than one backend (LAY-SHELL), and also when it has none
 * to run, so a single missing backend is shown with its status rather than hidden (FLOW-NEW-SESSION-R07).
 */
export function showsBackend(backends: readonly BackendChoices[]): boolean {
  return backends.length > 1 || available(backends).length === 0;
}

/** The choice marked default, else the first; absent when there is none. */
function defaultOf(choices: readonly Choice[]): string | undefined {
  return (choices.find((c) => c.default) ?? choices[0])?.id;
}

/** `wanted` when it is one of `choices`, else the default. */
function pick(choices: readonly Choice[], wanted: string | undefined): string | undefined {
  return choices.some((c) => c.id === wanted) ? wanted : defaultOf(choices);
}

/**
 * The values the session controls show for `project`: each value from `wanted` where the project and its host still offer
 * it, else the default. `wanted` is what the user last started a session with for the project, or what they have
 * chosen on the screen so far. The defaults: a new workspace where the project's version control has an adapter,
 * else the root; the first available backend; that backend's default model, effort and permission mode, which for
 * permission mode is the one that asks before every action outside the allow rules (FLOW-NEW-SESSION-R02, R06).
 */
export function valuesFor(project: Project, backends: readonly BackendChoices[], wanted?: Partial<SessionValues>): SessionValues {
  const kinds = workspaceKinds(project);
  const workspace = wanted?.workspace && kinds.includes(wanted.workspace) ? wanted.workspace : (kinds[0] ?? "root");
  const runnable = available(backends);
  const backend = runnable.find((b) => b.name === wanted?.backend) ?? runnable[0];
  // A value chosen for another backend means nothing to this one.
  const same = backend !== undefined && backend.name === wanted?.backend;
  return {
    host_id: project.host_id,
    project_id: project.id,
    workspace,
    backend: backend?.name,
    model: backend && pick(backend.models, same ? wanted?.model : undefined),
    effort: backend && pick(backend.efforts, same ? wanted?.effort : undefined),
    permission_mode: backend && pick(backend.permission_modes, same ? wanted?.permission_mode : undefined),
  };
}
