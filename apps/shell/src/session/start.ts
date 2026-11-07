// Starting a session from the new-session screen (FLOW-NEW-SESSION): nothing exists until the first message is
// sent; then the workspace (none for the root workspace), the session with the chosen values, and the message as
// its first turn. A start that fails takes back what it made and says why in the backend's own words.
// Kept free of React so the tests drive it with a fake client.

import { t } from "../catalog";
import { type BridgeError, type CoreClient, describeError } from "../core/client";
import type { Attachment, Session, SessionCreateParams, Workspace } from "../protocol/core-api.gen";
import { type Sending, sendFailure } from "./follow";
import type { BackendChoices, SessionValues, WorkspaceKind } from "./controls";

/** What the new-session screen offers on a host: its backends, and the values each project last started with. */
export interface Choices {
  /** None when `backend.list` failed. */
  backends: BackendChoices[];
  lastUsed: Record<string, SessionValues>;
}

/**
 * Reads what the new-session screen offers for `hostId`. Only reads: opening the screen creates nothing
 * (FLOW-NEW-SESSION-R03). Remembered values that cannot be read leave the screen at the defaults.
 */
export async function readChoices(client: Pick<CoreClient, "call">, hostId: string): Promise<Choices> {
  const [backends, settings] = await Promise.allSettled([client.call("backend.list", { host_id: hostId }), client.call("settings.get")]);
  return {
    backends: backends.status === "fulfilled" ? backends.value : [],
    lastUsed: settings.status === "fulfilled" ? lastUsedFrom(settings.value) : {},
  };
}

/** Why the first message did not go through to a session that is still alive; the session stays and opens. */
export interface FirstSendFailure {
  /** Refused with why, naming the attachment refused (R10), or a message Ersa can't tell arrived (FLOW-SEND-MESSAGE-R15). */
  sending: Exclude<Sending, { state: "sending" }>;
  /** Whether to give the text back to write again: not when Core says the backend already has it. */
  giveBack: boolean;
}

export type StartOutcome =
  | { ok: true; session: Session; workspace: Workspace; firstSend?: FirstSendFailure }
  | { ok: false; message: string }
  /** Stop was chosen before the message reached the agent: nothing is left, and the message stays with the user. */
  | { ok: false; stopped: true };

/**
 * Stop for a start under way (FLOW-SEND-MESSAGE-R03): `stop()` cancels the first message if it has not reached the
 * agent yet, as while the new session's backend is starting. The start then takes back what it made.
 */
export class StartStop {
  stopped = false;
  /** The session the start created, once it exists: where its first message's progress comes from (FLOW-SEND-MESSAGE-R16). */
  session?: string;
  constructor(private readonly client: Pick<CoreClient, "call">) {}
  /** The session the start created, so a stop can reach it. */
  created(id: string) {
    this.session = id;
  }
  stop() {
    this.stopped = true;
    // Core cancels a message not yet handed to the backend (PROTO-CORE-API-R25); before the session exists, the
    // start sees `stopped` itself.
    if (this.session) this.client.call("session.interrupt", { id: this.session }).catch(() => {});
  }
}

/** Thrown inside `startSession` when a stop ends the start, so its cleanup runs. */
const STOPPED = Symbol("stopped");

/** The states a session that never ran ends in: its backend failed to start, or it was stopped while starting. */
const NEVER_RAN: readonly Session["state"][] = ["error", "ended"];

/**
 * Starts a session with `values` and sends `text` as its first turn (FLOW-NEW-SESSION-R04, R05). When the session
 * could not be created, or Core says it never ran, it stops and removes the session and removes the workspace it
 * created, never the root, and reports the first failure's message, which for a backend that did not start is the
 * backend's own (R08). A session that is alive is kept whatever the first message's answer was: a slow start or a
 * message the backend already has is not a failed start, and the session opens with the error.
 */
export async function startSession(
  client: Pick<CoreClient, "call">,
  values: SessionValues,
  text: string,
  stop?: StartStop,
  attachments: Attachment[] = [],
): Promise<StartOutcome> {
  const { backend } = values;
  if (!backend) return { ok: false, message: t("composer.noBackendError") };

  let made: Workspace | undefined;
  let session: Session | undefined;
  try {
    let workspace: Workspace;
    if (values.workspace === "isolated") {
      // From the chosen base branch; with none, Core starts at the default branch (FLOW-NEW-SESSION-R21).
      workspace = made = await client.call("workspace.create", values.base ? { project_id: values.project_id, base: values.base } : { project_id: values.project_id });
    } else {
      const root = (await client.call("workspace.list", { project_id: values.project_id })).find((w) => w.kind === "root");
      if (!root) return { ok: false, message: "This project has no root workspace in Core." };
      workspace = root;
    }
    const params: SessionCreateParams = { workspace_id: workspace.id, backend };
    // A value the backend offers no choice of is left out: the backend decides.
    if (values.model) params.model = values.model;
    if (values.effort) params.effort = values.effort;
    if (values.permission_mode) params.permission_mode = values.permission_mode;
    session = await client.call("session.create", params);
    const created = session;
    stop?.created(created.id);
    // Stopped before the session existed: nothing is sent.
    if (stop?.stopped) throw STOPPED;
    try {
      await client.call("session.send", attachments.length > 0 ? { id: created.id, text, attachments } : { id: created.id, text });
      return { ok: true, session: created, workspace };
    } catch (error) {
      // A stop canceled the message before it reached the agent, which Core answers as an invalid state: take back
      // what was made. Any other failure is judged below, so a session the backend already ran is kept.
      const canceled = (error as BridgeError | undefined)?.kind === "rpc" && (error as Extract<BridgeError, { kind: "rpc" }>).error.data?.kind === "invalid_state";
      if (stop?.stopped && canceled) throw STOPPED;
      // Core answers a message to a backend that failed to start once the session's run has ended, so its state
      // already says so. A session Core cannot be asked about is kept: removing a live one loses its work. After
      // an `internal` answer the backend already has the message, so the session ran: it is kept whatever its
      // state says now, since a backend that started and then exited reads as `error` too.
      const internal = (error as BridgeError | undefined)?.kind === "rpc" && (error as Extract<BridgeError, { kind: "rpc" }>).error.data?.kind === "internal";
      const listed = await client.call("session.list").then(
        (all) => all.find((s) => s.id === created.id),
        () => undefined,
      );
      // A message Ersa can't tell arrived may be the backend's now, as one handed over just before the backend
      // stopped: its session is kept whatever its state, and the message is said to be unknown, never failed (R15).
      const failure = sendFailure(error);
      if (internal || failure.state === "unknown" || !listed || !NEVER_RAN.includes(listed.state)) {
        return { ok: true, session: listed ?? created, workspace, firstSend: { sending: failure, giveBack: !internal } };
      }
      throw error;
    }
  } catch (error) {
    // Best effort, in order: Core refuses to remove a running session, or a workspace that still has one.
    if (session) {
      const id = session.id;
      await client.call("session.stop", { id }).catch(() => {});
      await client.call("session.remove", { id }).catch(() => {});
    }
    if (made) await client.call("workspace.remove", { id: made.id }).catch(() => {});
    if (error === STOPPED) return { ok: false, stopped: true };
    return { ok: false, message: describeError(error) };
  }
}

/** Where Core's settings keep the values each project's last session started with: `new_session.last_used`. */
const KEY = "new_session";

/** Each value's key, so the patch names every one. */
const FIELDS = ["host_id", "project_id", "workspace", "backend", "model", "effort", "permission_mode"] as const;

/**
 * What is remembered of the values a session started with, in this window as in Core's settings: the values FIELDS
 * names, so never the base branch, since each new session starts at the default branch (FLOW-NEW-SESSION-R21).
 */
export function remembered(values: SessionValues): SessionValues {
  return Object.fromEntries(FIELDS.filter((f) => values[f] !== undefined).map((f) => [f, values[f]])) as unknown as SessionValues;
}

/**
 * The settings patch that remembers `values` as the last its project started a session with (FLOW-NEW-SESSION-R02).
 * Core merges a patch key by key (RFC 7386), so a value left out is sent as null: otherwise the previous start's
 * model or effort would stay beside a backend that offers none.
 */
export function rememberPatch(values: SessionValues): { [KEY]: { last_used: Record<string, Record<string, string | null>> } } {
  const entry = Object.fromEntries(FIELDS.map((f) => [f, values[f] ?? null]));
  return { [KEY]: { last_used: { [values.project_id]: entry } } };
}

const KINDS: readonly WorkspaceKind[] = ["isolated", "root"];
const optional = (v: unknown) => (typeof v === "string" ? v : undefined);

/**
 * The remembered values in `settings`, by project id. Settings are the user's and may hold anything, so an entry
 * that does not read as values is skipped; the screen then starts that project at the defaults.
 */
export function lastUsedFrom(settings: unknown): Record<string, SessionValues> {
  const stored = (settings as { [KEY]?: { last_used?: unknown } } | null)?.[KEY];
  const byProject = typeof stored === "object" && stored !== null ? stored.last_used : undefined;
  if (typeof byProject !== "object" || byProject === null) return {};
  const out: Record<string, SessionValues> = {};
  for (const [project_id, raw] of Object.entries(byProject)) {
    const v = raw as Record<string, unknown> | null;
    if (typeof v !== "object" || v === null || typeof v.host_id !== "string") continue;
    if (!KINDS.includes(v.workspace as WorkspaceKind)) continue;
    out[project_id] = {
      host_id: v.host_id,
      project_id,
      workspace: v.workspace as WorkspaceKind,
      backend: optional(v.backend),
      model: optional(v.model),
      effort: optional(v.effort),
      permission_mode: optional(v.permission_mode),
    };
  }
  return out;
}
