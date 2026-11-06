// The CoreClient the window uses: the Shell's two bridge commands
// (documented on the Shell's Rust side, in src-tauri/src/lib.rs). Beside it,
// the Shell's own controls for closing and quitting, and the system's folder
// dialog.

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { Attachment, CoreClient, CoreEvent } from "./client";
import type { ShellControl } from "./shell";

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

export const tauriShell: ShellControl = {
  onQuitRequested: (handler) => {
    // `listen` registers asynchronously; stopping before it has means
    // unlistening as soon as it does.
    let stopped = false;
    let unlisten: (() => void) | undefined;
    listen("quit-requested", () => handler()).then((stop) => {
      if (stopped) stop();
      else unlisten = stop;
    });
    return () => {
      stopped = true;
      unlisten?.();
    };
  },
  quitHeard: () => invoke("shell_quit_heard"),
  show: () => invoke<boolean>("shell_show"),
  hide: () => invoke("shell_hide"),
  quit: () => invoke("shell_quit"),
  // One folder, or null when the user cancels (FLOW-NEW-SESSION-R27, R17).
  pickFolder: async () => {
    const picked = await open({ directory: true, multiple: false });
    return typeof picked === "string" ? picked : null;
  },
};
