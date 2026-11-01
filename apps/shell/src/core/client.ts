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
  /** Core's answer to `core.hello` on this connection. */
  hello: CoreHelloResult;
}

/** Why a request got no result, as the Shell reports it. */
export type BridgeError =
  | { kind: "rpc"; error: RpcError }
  | { kind: "unavailable"; message: string };

export type Method = keyof CoreApiMethods;
export type Params<M extends Method> = CoreApiMethods[M]["params"];
export type Result<M extends Method> = CoreApiMethods[M]["result"];

export type AnyNotification = {
  [N in keyof CoreApiNotifications]: { method: N; params: CoreApiNotifications[N] };
}[keyof CoreApiNotifications];

export interface CoreClient {
  /** Attaches to Core, launching it if none is running, and completes the handshake. */
  attach(): Promise<Attachment>;
  /** One Core API request. Rejects with a {@link BridgeError}. */
  call<M extends Method>(method: M, params?: Params<M>): Promise<Result<M>>;
  /** Runs `listener` when the connection to Core drops. Returns an unsubscribe function. */
  onConnectionLost(listener: (reason: string) => void): () => void;
  /** Runs `listener` for every notification Core sends. Returns an unsubscribe function. */
  onNotification(listener: (notification: AnyNotification) => void): () => void;
}

/** A readable message for anything a client call rejected with. */
export function describeError(error: unknown): string {
  if (typeof error === "object" && error !== null && "kind" in error) {
    const bridge = error as BridgeError;
    if (bridge.kind === "rpc") return bridge.error.data?.detail ?? bridge.error.message;
    if (bridge.kind === "unavailable") return bridge.message;
  }
  if (error instanceof globalThis.Error) return error.message;
  return String(error);
}
