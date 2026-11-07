// The replay server's streams, as the Interface receives them, captured by
// the Shell's replay test (src-tauri/tests/replay.rs), which fails when they
// no longer match what the replay server sends. For tests only.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { Envelope } from "../protocol/agent-events.gen";
import type { Session } from "../protocol/core-api.gen";

export interface Capture {
  /** `session.list` on attach, and once the replay has played out. */
  sessions: Session[];
  sessions_after: Session[];
  /** The commit the capture was written from, `-dirty` if uncommitted. */
  ersa_commit?: string;
  notifications: { kind: "notification"; method: "session.event"; params: { session_id: string; event: Envelope } }[];
}

export type Replay = "02-partial-messages" | "07-attachments" | "11-stream-permission-prompt-tool-stdio";

export function capture(name: Replay): Capture {
  return JSON.parse(readFileSync(join(import.meta.dirname, "replays", `${name}.json`), "utf8")) as Capture;
}

/** The capture's events, in the order they arrived. */
export function events(name: Replay): Envelope[] {
  return capture(name).notifications.map((n) => n.params.event);
}
