// What the window knows about Core, and how it gets there: attach (or
// launch), handshake, then read Core's status, the project list and the
// session list, keeping the sessions' tiles current from `session.tile`;
// when the connection drops, attach again on its own with backoff.
// Kept free of React so the tests drive it with a fake client.

import type { CoreHelloResult, CoreStatusResult, Project, Session } from "../protocol/core-api.gen";
import { type AnyNotification, type Attachment, type CoreClient, type CoreEvent, describeError } from "./client";

export type ProjectsView =
  | { state: "loading" }
  | { state: "ready"; projects: Project[] }
  | { state: "error"; message: string }
  /** Core is not connected; its status tile already says why. */
  | { state: "unavailable" };

export type SessionsView =
  | { state: "loading" }
  | { state: "ready"; sessions: Session[] }
  | { state: "error"; message: string }
  | { state: "unavailable" };

export type CoreView =
  | { phase: "connecting" }
  | {
      phase: "connected";
      /** Counts attaches: a new number means a new connection, on which subscriptions start again. */
      link: number;
      how: Attachment["how"];
      /** The endpoint's label, when the window is attached to one other than the user's own. */
      label?: string;
      hello: CoreHelloResult;
      /** Absent until `core.status` answers, or if it failed. */
      status?: CoreStatusResult;
      projects: ProjectsView;
      sessions: SessionsView;
    }
  /** Was connected; the connection dropped, and attaching again is under way. */
  | { phase: "lost"; reason: string; hello: CoreHelloResult }
  /** Could not attach or launch at all; the user decides when to try again. */
  | { phase: "failed"; message: string };

/** How long to wait before each attempt to attach again after a loss. The last repeats. */
export const RECONNECT_DELAYS_MS = [500, 1_000, 2_000, 4_000, 8_000, 15_000, 30_000];

/** A running connection: read a list again after changing it, or stop. */
export interface Connection {
  /** Reads the list again, if connected; Core does not announce new projects or sessions. */
  reload(list: "projects" | "sessions"): void;
  /** After this, neither `update` nor `onNotification` is called again. */
  stop(): void;
}

/**
 * Connects and reports each step through `update`, and passes every
 * notification from the current connection to `onNotification`.
 */
export function connect(
  client: CoreClient,
  update: (view: CoreView) => void,
  delays: readonly number[] = RECONNECT_DELAYS_MS,
  onNotification: (notification: AnyNotification) => void = () => {},
): Connection {
  let current: CoreView = { phase: "connecting" };
  let stopped = false;
  // Each attach gets a number; answers and events from an older one are ignored.
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  // The current attach's way to read each list again; nothing until connected.
  let reload: (list: "projects" | "sessions") => void = () => {};

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

    // Only change the view while this attach is still the connected one,
    // so a late answer cannot undo a lost connection.
    const patch = (change: (view: Extract<CoreView, { phase: "connected" }>) => CoreView) => {
      if (live() && current.phase === "connected") set(change(current));
    };

    // Each read of the project list gets a number, so an older answer that
    // arrives late cannot replace a newer one.
    const reads = { projects: 0 };
    const listProjects = () => {
      const read = ++reads.projects;
      const latest = () => read === reads.projects;
      client.call("project.list").then(
        (projects) => latest() && patch((view) => ({ ...view, projects: { state: "ready", projects } })),
        (error) => latest() && patch((view) => ({ ...view, projects: { state: "error", message: describeError(error) } })),
      );
    };
    // One session read at a time: asking again while one is in flight
    // reads once more after it, so a burst of tiles costs two reads, not one each.
    let sessionRead: "idle" | "reading" | "again" = "idle";
    const listSessions = () => {
      if (sessionRead !== "idle") {
        sessionRead = "again";
        return;
      }
      sessionRead = "reading";
      const done = () => {
        const again = sessionRead === "again";
        sessionRead = "idle";
        if (again && live()) listSessions();
      };
      client.call("session.list").then(
        (sessions) => {
          patch((view) => ({ ...view, sessions: { state: "ready", sessions } }));
          done();
        },
        (error) => {
          patch((view) => ({ ...view, sessions: { state: "error", message: describeError(error) } }));
          done();
        },
      );
    };

    const onEvent = (event: CoreEvent) => {
      if (!live()) return;
      if (event.kind === "notification") {
        if (event.method === "session.tile") {
          const { session_id, tile } = event.params;
          const known =
            current.phase === "connected" &&
            current.sessions.state === "ready" &&
            current.sessions.sessions.some((s) => s.id === session_id);
          // A tile for a session the list lacks means a session was created
          // since it was read: read it again.
          if (known) {
            // A read in flight may answer with a list older than this
            // tile; read once more after it so the newer tile is not lost.
            if (sessionRead !== "idle") listSessions();
            patch((view) =>
              view.sessions.state === "ready"
                ? {
                    ...view,
                    sessions: {
                      state: "ready",
                      sessions: view.sessions.sessions.map((s) => (s.id === session_id ? { ...s, tile } : s)),
                    },
                  }
                : view,
            );
          } else if (current.phase === "connected") {
            listSessions();
          }
        }
        onNotification(event);
        return;
      }
      if (current.phase === "connected") lose(event.reason, current.hello, 0);
      else lostEarly = event.reason;
    };

    client.attach(onEvent).then(
      ({ how, label, hello }) => {
        if (!live()) return;
        if (lostEarly !== null) {
          lose(lostEarly, hello, retry);
          return;
        }
        set({
          phase: "connected",
          link: mine,
          how,
          ...(label === undefined ? {} : { label }),
          hello,
          projects: { state: "loading" },
          sessions: { state: "loading" },
        });
        client.call("core.status").then(
          (status) => patch((view) => ({ ...view, status })),
          // The tile shows the handshake without the counts; nothing to report.
          () => {},
        );
        listProjects();
        listSessions();
        reload = (list) => (list === "projects" ? listProjects() : listSessions());
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

  return {
    reload: (list) => reload(list),
    stop: () => {
      stopped = true;
      clearTimeout(timer);
    },
  };
}
