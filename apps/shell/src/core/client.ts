// The Interface's only way to reach Core (ARCH-CORE-R02): Core API requests
// and notifications, typed by the types generated from the protocol schemas.
// The Shell carries them over the pipe; the Interface never opens it.

import { t } from "../catalog";
import type {
  CoreApiMethods,
  CoreApiNotifications,
  CoreHelloResult,
  ErrorData,
  Error as RpcError,
} from "../protocol/core-api.gen";

/** How the Shell reached Core: found it running, or started it. */
export interface Attachment {
  how: "attached" | "launched";
  /**
   * The endpoint's label when the Shell attached to one other than the
   * user's own (a debug build told to by `ERSA_ATTACH`, such as a replay server).
   */
  label?: string;
  /** Core's answer to `core.hello` on this connection. */
  hello: CoreHelloResult;
}

/** Why a request got no result, as the Shell reports it. */
export type BridgeError =
  | { kind: "rpc"; error: RpcError }
  /** `sent`: the request was queued for Core before it failed, so Core may have acted on it. */
  | { kind: "unavailable"; message: string; sent?: true };

export type Method = keyof CoreApiMethods;
export type Params<M extends Method> = CoreApiMethods[M]["params"];
export type Result<M extends Method> = CoreApiMethods[M]["result"];

export type AnyNotification = {
  [N in keyof CoreApiNotifications]: { method: N; params: CoreApiNotifications[N] };
}[keyof CoreApiNotifications];

/** What the Shell tells the Interface without being asked, in the order it happened. */
export type CoreEvent = ({ kind: "notification" } & AnyNotification) | { kind: "lost"; reason: string };

export interface CoreClient {
  /**
   * Attaches to Core, launching it if none is running, and completes the
   * handshake. `onEvent` receives every notification and the connection's
   * loss from the moment of the call, until the next attach replaces it.
   */
  attach(onEvent: (event: CoreEvent) => void): Promise<Attachment>;
  /** One Core API request. Rejects with a {@link BridgeError}. */
  call<M extends Method>(method: M, params?: Params<M>): Promise<Result<M>>;
}

/** Core's refusal, when a client call was refused by Core rather than failing on the way: its kind and detail. */
export function refusalOf(error: unknown): ErrorData | undefined {
  if (typeof error !== "object" || error === null || !("kind" in error) || error.kind !== "rpc" || !("error" in error)) return undefined;
  return (error as { error: RpcError }).error.data;
}

type Reason = NonNullable<ErrorData["reason"]>;

// The catalog's words for each refusal reason the Interface knows (PROTO-CORE-API-R80). `folder_gone` has none yet:
// its words, and where they show, come with the ticket for Copy session ID; until then it reads as Core's message.
const REFUSAL_WORDS: Partial<Record<Reason, () => string>> = {
  session_busy: () => t("errors.refusal.sessionBusyError"),
  session_deleting: () => t("errors.refusal.sessionDeletingError"),
  session_archived: () => t("errors.refusal.sessionArchivedError"),
  session_not_running: () => t("errors.refusal.sessionNotRunningError"),
  send_in_progress: () => t("errors.refusal.sendInProgressError"),
  send_stopped: () => t("errors.refusal.sendStoppedError"),
  backend_unnamed: () => t("errors.refusal.backendUnnamedError"),
  record_gone: () => t("errors.refusal.recordGoneError"),
  open_unknown: () => t("errors.refusal.openUnknownError"),
  no_finished_reply: () => t("errors.refusal.noFinishedReplyError"),
  backend_cannot_fork: () => t("errors.refusal.backendCannotForkError"),
};

// Process ids as Core lists them for `open_elsewhere`: decimal numbers separated by a comma and a space (R87).
const PROCESS_IDS = /^\d+(, \d+)*$/;

/**
 * The catalog's words for a refusal whose `reason` the Interface knows, else undefined. `open_elsewhere` names the
 * processes in `detail` (PROTO-CORE-API-R87); a detail of any other shape is never shown. A send refused because its
 * backend could not be started has no reason (R88), so it is said in the adapter's words, as is a reason this build
 * doesn't know.
 */
function refusalWords(data: ErrorData | undefined): string | undefined {
  const reason = data?.reason;
  if (reason === undefined) return undefined;
  if (reason === "open_elsewhere") {
    const detail = data?.detail ?? "";
    if (!PROCESS_IDS.test(detail)) return t("errors.refusal.openElsewhereUnnamedError");
    return t("errors.refusal.openElsewhereError", { count: detail.split(", ").length, ids: detail });
  }
  // `hasOwn`, so a reason from a newer Core that happens to name an Object property finds nothing.
  return Object.hasOwn(REFUSAL_WORDS, reason) ? REFUSAL_WORDS[reason]?.() : undefined;
}

/** A readable message for anything a client call rejected with: a refusal Core names in the catalog's words. */
export function describeError(error: unknown): string {
  const words = refusalWords(refusalOf(error));
  if (words !== undefined) return words;
  if (typeof error === "object" && error !== null && "kind" in error) {
    const bridge = error as BridgeError;
    // Core's messages start with the method they answer ("session.send: …"),
    // which means nothing to the user.
    // Only Core's own message is trimmed; a detail may quote anything, file names included.
    if (bridge.kind === "rpc") return bridge.error.data?.detail ?? bridge.error.message.replace(/^[a-z_]+\.[a-z_]+: /, "");
    if (bridge.kind === "unavailable") return bridge.message;
  }
  if (error instanceof globalThis.Error) return error.message;
  return String(error);
}
