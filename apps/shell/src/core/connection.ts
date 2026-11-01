// What the window knows about Core, and how it gets there: attach (or
// launch), handshake, then read Core's status and the project list.
// Kept free of React so the tests drive it with a fake client.

import type { CoreHelloResult, CoreStatusResult, Project } from "../protocol/core-api.gen";
import { type Attachment, type CoreClient, describeError } from "./client";

export type ProjectsView =
  | { state: "loading" }
  | { state: "ready"; projects: Project[] }
  | { state: "error"; message: string };

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
  /** Was connected; the connection dropped. */
  | { phase: "lost"; reason: string; hello: CoreHelloResult }
  /** Could not attach or launch at all. */
  | { phase: "failed"; message: string };

/**
 * Connects and reports each step through `update`. Returns a function that
 * stops listening; after it is called, `update` is never called again.
 */
export function connect(client: CoreClient, update: (view: CoreView) => void): () => void {
  let current: CoreView = { phase: "connecting" };
  let stopped = false;
  const set = (view: CoreView) => {
    if (stopped) return;
    current = view;
    update(view);
  };
  // Only apply a change while still connected, so a late answer cannot undo
  // a lost connection.
  const patch = (change: (view: Extract<CoreView, { phase: "connected" }>) => CoreView) => {
    if (current.phase === "connected") set(change(current));
  };

  set(current);
  const unlisten = client.onConnectionLost((reason) => {
    if (current.phase === "connected") set({ phase: "lost", reason, hello: current.hello });
  });

  client.attach().then(
    ({ how, hello }) => {
      set({ phase: "connected", how, hello, projects: { state: "loading" } });
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
    (error) => set({ phase: "failed", message: describeError(error) }),
  );

  return () => {
    stopped = true;
    unlisten();
  };
}
