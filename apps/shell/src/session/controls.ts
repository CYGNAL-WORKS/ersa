// The session controls' values: what a new session will start with (FLOW-NEW-SESSION, LAY-SHELL): host,
// project, workspace, backend, model, effort and permission mode. The choices come from Core's `project.list`
// and `backend.list`; the starting values from what the user last chose for the project, else the brief's defaults.

import { t } from "../catalog";
import type { BackendInfo, Branch, Choice, Project, Session, Workspace } from "../protocol/core-api.gen";

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
  /**
   * The branch a new workspace starts from (FLOW-NEW-SESSION-R21). Never remembered: each new session starts at the
   * repository's default branch. Absent, Core starts the workspace at the default branch, else the checked-out commit.
   */
  base?: string;
}

/** A project's local branches from `branch.list`, as the workspace chip offers them: being read, read, or not readable. */
export type Branches = { state: "reading" } | { state: "read"; branches: Branch[] } | { state: "failed" };

/**
 * The base branch the workspace chip shows (FLOW-NEW-SESSION-R21): `wanted` while the project still has it, else the
 * repository's default branch; none while the branches are unread or none is the default.
 */
export function baseFor(branches: readonly Branch[] | undefined, wanted?: string): string | undefined {
  if (!branches) return undefined;
  return branches.find((b) => b.name === wanted)?.name ?? branches.find((b) => b.default)?.name;
}

/** The branches whose names hold `query`, ignoring case and the spaces around it, in the order listed. */
export function branchesMatching(branches: readonly Branch[], query: string): Branch[] {
  const wanted = query.trim().toLowerCase();
  return branches.filter((b) => b.name.toLowerCase().includes(wanted));
}

/** The version control Ersa can make isolated workspaces for. Others run in the root workspace only. */
const WORKSPACE_ADAPTERS: readonly Project["vcs"][] = ["git"];

/** The workspaces a project offers: a new isolated one first where Ersa has an adapter, and always the root. */
export function workspaceKinds(project: Project): WorkspaceKind[] {
  return WORKSPACE_ADAPTERS.includes(project.vcs) ? ["isolated", "root"] : ["root"];
}

/**
 * A time Core sent, as a number to compare. Core may pass on a backend's time with a fraction of any length, so the
 * text alone does not order them.
 */
export const timeOf = (at: string): number => Date.parse(at) || 0;

/** The sessions newest first, by `created_at`: the rail's default order, and the project list's (FLOW-NEW-SESSION-R14). */
export function newestFirst<S extends Pick<Session, "created_at">>(sessions: readonly S[]): S[] {
  return sessions.slice().sort((a, b) => timeOf(b.created_at) - timeOf(a.created_at));
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
  // Walking the sessions newest first, a project's first session met is its newest, so the order met is the order.
  const rank = new Map<string, number>();
  for (const s of newestFirst(sessions)) {
    const project = workspaces[s.workspace_id]?.project_id;
    if (project !== undefined && !rank.has(project)) rank.set(project, rank.size);
  }
  // Array.prototype.sort is stable, so unused projects keep Core's order.
  return [...projects].sort((a, b) => (rank.get(a.id) ?? Infinity) - (rank.get(b.id) ?? Infinity) || 0);
}

/**
 * The project list's order as the window shows it (FLOW-NEW-SESSION-R14): `recentFirst` by `listed`, the sessions
 * `session.list` gave, or, while that list is reloading or Core is away (`listed` absent), by `kept`, the sessions
 * it gave last, so the order and the project the screen starts at do not jump back to Core's order meanwhile.
 * Returns the order and the sessions to keep for next time.
 */
export function projectOrder(
  projects: readonly Project[],
  listed: readonly Session[] | undefined,
  kept: readonly Session[],
  workspaces: Readonly<Record<string, Pick<Workspace, "project_id">>>,
): { projects: Project[]; kept: readonly Session[] } {
  const by = listed ?? kept;
  return { projects: recentFirst(projects, by, workspaces), kept: by };
}

/** Where the new-session screen starts, and what the user has chosen on it since. */
export interface ScreenSeed {
  /** The project New session was chosen for, if any. */
  shownFor?: string;
  /** The last-used values the screen started from. */
  seededWith?: SessionValues;
  /** The user has chosen, written or sent on the screen: nothing re-seeds it after that. */
  touched: boolean;
  /** What the user has chosen; the controls show it where the project and its host offer it. */
  wanted: Partial<SessionValues>;
}

/**
 * The screen's seed as of this render: `prev` while nothing calls for a fresh start, else a fresh start at `first`
 * with what was last used there (FLOW-NEW-SESSION-R02). A fresh start comes when New session is chosen for another
 * project, and, until the user touches the screen, when last-used values arrive or the starting project itself
 * changes: the sessions and workspaces that order the projects arrive after the screen opens at launch
 * (FLOW-NEW-SESSION-R15). Returns `prev` itself when it stands, so a caller can tell nothing changed.
 */
export function seedScreen(
  prev: ScreenSeed | undefined,
  first: Project | undefined,
  projectId: string | undefined,
  lastUsed: Readonly<Record<string, SessionValues>>,
): ScreenSeed {
  const fresh: () => ScreenSeed = () => ({
    shownFor: projectId,
    seededWith: first && lastUsed[first.id],
    touched: false,
    wanted: first ? { ...lastUsed[first.id], project_id: first.id } : {},
  });
  if (prev === undefined || prev.shownFor !== projectId) return fresh();
  if (!prev.touched && first && (lastUsed[first.id] !== prev.seededWith || first.id !== prev.wanted.project_id)) return fresh();
  return prev;
}

/**
 * The project the new-session screen starts at: the one New session was chosen for, else the most recently used
 * project on `hostId` (FLOW-NEW-SESSION-R15), else the most recently used of all. `ordered` is `recentFirst`'s order.
 */
export function startingProject(ordered: readonly Project[], hostId: string, projectId?: string): Project | undefined {
  return ordered.find((p) => p.id === projectId) ?? ordered.find((p) => p.host_id === hostId) ?? ordered[0];
}

/** The backends a session can start with: those detected as available. */
export function available(backends: readonly BackendInfo[]): BackendInfo[] {
  return backends.filter((b) => b.status === "available");
}

/**
 * Whether the backend chip shows: when the host has more than one backend (LAY-SHELL), and also when it has none
 * to run, so a single missing backend is shown with its status rather than hidden (FLOW-NEW-SESSION-R07).
 */
export function showsBackend(backends: readonly BackendInfo[]): boolean {
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
export function valuesFor(project: Project, backends: readonly BackendInfo[], wanted?: Partial<SessionValues>): SessionValues {
  const kinds = workspaceKinds(project);
  const workspace = wanted?.workspace && kinds.includes(wanted.workspace) ? wanted.workspace : (kinds[0] ?? "root");
  return { host_id: project.host_id, project_id: project.id, workspace, ...howFor(backends, wanted) };
}

/** How a session runs, as `valuesFor` chooses it: the backend and its model, effort and permission mode. */
type HowValues = Pick<SessionValues, "backend" | "model" | "effort" | "permission_mode">;

/**
 * The how values alone, from the host's backends: what `valuesFor` gives, and what the session controls show while
 * the host has no project yet (FLOW-NEW-SESSION-R19).
 */
export function howFor(backends: readonly BackendInfo[], wanted?: Partial<SessionValues>): HowValues {
  const runnable = available(backends);
  const backend = runnable.find((b) => b.name === wanted?.backend) ?? runnable[0];
  // A value chosen for another backend means nothing to this one.
  const same = backend !== undefined && backend.name === wanted?.backend;
  return {
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
  backend: BackendInfo;
  /** The workspace as the session runs in it: its branch, or the project folder. */
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
  hostBackends: readonly BackendInfo[],
): SessionShown {
  const listed = hostBackends.find((b) => b.name === session.backend);
  const model = session.model ?? (listed && defaultOf(listed.models));
  const effort = session.effort ?? (listed && defaultOf(listed.efforts));
  const backend: BackendInfo = {
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
    // The root workspace by the name the new-session screen gives it (GLOSSARY: the project folder).
    workspaceLabel: workspace.kind === "root" ? t("sessionControls.workspace.projectFolderLabel") : workspace.vcs_ref,
  };
}
