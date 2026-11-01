// The CoreClient the window uses: the Shell's bridge commands and events
// (documented on the Shell's Rust side, in src-tauri/src/lib.rs).

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AnyNotification, Attachment, CoreClient } from "./client";

/** Whether this page runs inside the Shell's webview, rather than a plain browser. */
export function inShell(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

/** Subscribes through Tauri's async `listen`, returning a synchronous unsubscribe. */
function subscribe<T>(event: string, handler: (payload: T) => void): () => void {
  let stop: (() => void) | null = null;
  let stopped = false;
  void listen<T>(event, (e) => handler(e.payload)).then((unlisten) => {
    if (stopped) unlisten();
    else stop = unlisten;
  });
  return () => {
    stopped = true;
    stop?.();
  };
}

export const tauriClient: CoreClient = {
  attach: () => invoke<Attachment>("core_attach"),
  call: (method, params) => invoke("core_request", { method, params: params ?? null }),
  onConnectionLost: (listener) =>
    subscribe<{ state: "lost"; reason: string }>("core-connection", (p) => listener(p.reason)),
  onNotification: (listener) => subscribe<AnyNotification>("core-notification", listener),
};
