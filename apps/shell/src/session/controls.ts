// The session controls' values: what a new session will start with (FLOW-NEW-SESSION, LAY-SHELL): host,
// project, workspace, backend, model, effort and permission mode. The choices come from Core's `project.list`
// and `backend.list`; the starting values from what the user last chose for the project, else the brief's defaults.

import type { Project, Session, Workspace } from "../protocol/core-api.gen";

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
  /** What the interface shows: the backend's own name, such as "Claude Code". */
  label: string;
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

/**
 * The projects most recently used first (FLOW-NEW-SESSION-R14): by the creation time of each project's newest session
 * in `session.list`, a session's project read through its workspace. A project with no session it can be traced to
 * comes after the used ones, and those keep the order Core lists them in.
 */
export function recentFirst(
  projects: readonly Project[],
  sessions: readonly Pick<Session, "workspace_id" | "created_at">[],
  workspaces: Readonly<Record<string, Pick<Workspace, "project_id">>>,
): Project[] {
  // Each project's newest session, as milliseconds: parsed, so times with different offsets compare as instants.
  const newest = new Map<string, number>();
  for (const s of sessions) {
    const project = workspaces[s.workspace_id]?.project_id;
    const at = Date.parse(s.created_at);
    if (project === undefined || Number.isNaN(at)) continue;
    newest.set(project, Math.max(at, newest.get(project) ?? at));
  }
  // Array.prototype.sort is stable, so projects that tie keep Core's order.
  return [...projects].sort((a, b) => (newest.get(b.id) ?? -Infinity) - (newest.get(a.id) ?? -Infinity) || 0);
}

/**
 * The project the new-session screen starts at: the one New session was chosen for, else the most recently used
 * project on `hostId` (FLOW-NEW-SESSION-R15), else the most recently used of all. `ordered` is `recentFirst`'s order.
 */
export function startingProject(ordered: readonly Project[], hostId: string, projectId?: string): Project | undefined {
  return ordered.find((p) => p.id === projectId) ?? ordered.find((p) => p.host_id === hostId) ?? ordered[0];
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

/** What the session controls under a running session show: its own values, and the choices that name them. */
export interface SessionShown {
  values: SessionValues;
  /** The session's backend as `backend.list` lists it, holding every value the session reports. */
  backend: BackendChoices;
  /** The workspace as the session runs in it: its branch, or the root. */
  workspaceLabel: string;
}

/** `choices`, holding `id` too when the backend no longer offers it, under its id. */
function holding(choices: readonly Choice[], id: string | undefined): Choice[] {
  return id === undefined || choices.some((c) => c.id === id) ? [...choices] : [...choices, { id, label: id, default: false }];
}

/**
 * The values a running session shows (FLOW-NEW-SESSION-R12): what `session.list` reports, never what the host offers
 * now. A model or effort the session was not given is the backend's own, shown as the choice the backend marks as
 * its default when the host lists it. A value the host no longer lists shows under its id.
 */
export function sessionShown(
  session: Pick<Session, "host_id" | "backend" | "model" | "effort" | "permission_mode">,
  workspace: Pick<Workspace, "project_id" | "kind" | "vcs_ref">,
  hostBackends: readonly BackendChoices[],
): SessionShown {
  const listed = hostBackends.find((b) => b.name === session.backend);
  const model = session.model ?? (listed && defaultOf(listed.models));
  const effort = session.effort ?? (listed && defaultOf(listed.efforts));
  const backend: BackendChoices = {
    ...(listed ?? { name: session.backend, label: session.backend, status: "available", tested: { min: "", max: "" }, models: [], efforts: [], permission_modes: [] }),
  };
  backend.models = holding(backend.models, model);
  backend.efforts = holding(backend.efforts, effort);
  backend.permission_modes = holding(backend.permission_modes, session.permission_mode || undefined);
  return {
    values: {
      host_id: session.host_id,
      project_id: workspace.project_id,
      workspace: workspace.kind,
      backend: session.backend,
      model,
      effort,
      permission_mode: session.permission_mode || undefined,
    },
    backend,
    workspaceLabel: workspace.kind === "root" ? "Root workspace" : workspace.vcs_ref,
  };
}
