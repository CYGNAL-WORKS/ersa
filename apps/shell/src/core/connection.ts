// What the window knows about Core, and how it gets there: attach (or
// launch), handshake, then read Core's status and the project list; when the
// connection drops, attach again on its own with backoff.
// Kept free of React so the tests drive it with a fake client.

import type { CoreHelloResult, CoreStatusResult, Project } from "../protocol/core-api.gen";
import { type Attachment, type CoreClient, type CoreEvent, describeError } from "./client";

export type ProjectsView =
  | { state: "loading" }
  | { state: "ready"; projects: Project[] }
  | { state: "error"; message: string }
  /** Core is not connected; its status tile already says why. */
  | { state: "unavailable" };

export type CoreView =
  | { phase: "connecting" }
  | {
      phase: "connected";
      how: Attachment["how"];
      hello: CoreHelloResult;
      /** Absent until `core.status` answers, or if it failed. */
      status?: CoreStatusResult;
      projects: ProjectsView;
    }
  /** Was connected; the connection dropped, and attaching again is under way. */
  | { phase: "lost"; reason: string; hello: CoreHelloResult }
  /** Could not attach or launch at all; the user decides when to try again. */
  | { phase: "failed"; message: string };

/** How long to wait before each attempt to attach again after a loss. The last repeats. */
export const RECONNECT_DELAYS_MS = [500, 1_000, 2_000, 4_000, 8_000, 15_000, 30_000];

/**
 * Connects and reports each step through `update`. Returns a function that
 * stops; after it is called, `update` is never called again.
 */
export function connect(
  client: CoreClient,
  update: (view: CoreView) => void,
  delays: readonly number[] = RECONNECT_DELAYS_MS,
): () => void {
  let current: CoreView = { phase: "connecting" };
  let stopped = false;
  // Each attach gets a number; answers and events from an older one are ignored.
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const set = (view: CoreView) => {
    if (stopped) return;
    current = view;
    update(view);
  };

  const retryLater = (retry: number) => {
    const delay = delays[Math.min(retry, delays.length - 1)] ?? 0;
    timer = setTimeout(() => attach(retry + 1), delay);
  };

  const lose = (reason: string, hello: CoreHelloResult, retry: number) => {
    set({ phase: "lost", reason, hello });
    retryLater(retry);
  };

  /** Attaches; `retry` counts attempts since the last loss (0 for the first attach). */
  const attach = (retry: number) => {
    if (stopped) return;
    const mine = ++generation;
    const live = () => !stopped && mine === generation;
    // A loss can arrive before attach() resolves: the Shell reports it as
    // soon as the link drops, which may be during the handshake's reply.
    let lostEarly: string | null = null;

    const onEvent = (event: CoreEvent) => {
      if (!live() || event.kind !== "lost") return;
      if (current.phase === "connected") lose(event.reason, current.hello, 0);
      else lostEarly = event.reason;
    };

    client.attach(onEvent).then(
      ({ how, hello }) => {
        if (!live()) return;
        if (lostEarly !== null) {
          lose(lostEarly, hello, retry);
          return;
        }
        set({ phase: "connected", how, hello, projects: { state: "loading" } });
        // Only apply an answer while this attach is still the connected one,
        // so a late answer cannot undo a lost connection.
        const patch = (change: (view: Extract<CoreView, { phase: "connected" }>) => CoreView) => {
          if (live() && current.phase === "connected") set(change(current));
        };
        client.call("core.status").then(
          (status) => patch((view) => ({ ...view, status })),
          // The tile shows the handshake without the counts; nothing to report.
          () => {},
        );
        client.call("project.list").then(
          (projects) => patch((view) => ({ ...view, projects: { state: "ready", projects } })),
          (error) => patch((view) => ({ ...view, projects: { state: "error", message: describeError(error) } })),
        );
      },
      (error) => {
        if (!live()) return;
        // Reattaching after a loss keeps trying; a first attach that fails
        // waits for the user.
        if (current.phase === "lost") lose(describeError(error), current.hello, retry);
        else set({ phase: "failed", message: describeError(error) });
      },
    );
  };

  set(current);
  attach(0);

  return () => {
    stopped = true;
    clearTimeout(timer);
  };
}
