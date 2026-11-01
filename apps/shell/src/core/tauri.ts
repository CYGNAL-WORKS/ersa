// The CoreClient the window uses: the Shell's two bridge commands
// (documented on the Shell's Rust side, in src-tauri/src/lib.rs).

import { Channel, invoke } from "@tauri-apps/api/core";
import type { Attachment, CoreClient, CoreEvent } from "./client";

export const tauriClient: CoreClient = {
  attach: (onEvent) => {
    // The channel exists, with its handler, before the Shell sees it, so
    // nothing Core sends during the attach is missed.
    const events = new Channel<CoreEvent>();
    events.onmessage = onEvent;
    return invoke<Attachment>("core_attach", { events });
  },
  call: (method, params) => invoke("core_request", { method, params: params ?? null }),
};
