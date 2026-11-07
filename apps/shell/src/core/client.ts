// The Interface's only way to reach Core (ARCH-CORE-R02): Core API requests
// and notifications, typed by the types generated from the protocol schemas.
// The Shell carries them over the pipe; the Interface never opens it.

import type {
  CoreApiMethods,
  CoreApiNotifications,
  CoreHelloResult,
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
  /** `sent`: the request was written to Core before it failed, so Core may have acted on it. */
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

/** A readable message for anything a client call rejected with. */
export function describeError(error: unknown): string {
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
