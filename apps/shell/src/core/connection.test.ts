import { afterEach, describe, expect, it, vi } from "vitest";
import type { CoreHelloResult, Project } from "../protocol/core-api.gen";
import type { Attachment, BridgeError, CoreClient, CoreEvent, Method } from "./client";
import { type CoreView, connect } from "./connection";

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const project: Project = { id: "p1", host_id: "local", path: "D:/code/ersa", name: "ersa", vcs: "git" };

const status = { sessions_running: 2, sessions_waiting: 1, uptime_s: 5, db_path: "x" };

const launched = () => Promise.resolve<Attachment>({ how: "launched", hello });

/**
 * A client whose answers the test sets per method, whose attaches the test
 * scripts in order (the last repeats), and whose connection the test can drop.
 */
function fakeClient(answers: Partial<Record<Method, () => Promise<unknown>>>, attaches: (() => Promise<Attachment>)[] = [launched]) {
  let onEvent: (event: CoreEvent) => void = () => {};
  let attached = 0;
  const client: CoreClient = {
    attach: (listener) => {
      onEvent = listener;
      const next = attaches[Math.min(attached, attaches.length - 1)] ?? launched;
      attached++;
      return next();
    },
    call: (method) => (answers[method]?.() ?? Promise.reject({ kind: "unavailable", message: `no answer for ${method}` })) as never,
  };
  return {
    client,
    drop: (reason: string) => onEvent({ kind: "lost", reason }),
    attachCount: () => attached,
  };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

/** Runs `connect` until its promises settle and returns every view it reported. */
async function views(client: CoreClient): Promise<CoreView[]> {
  const seen: CoreView[] = [];
  connect(client, (v) => seen.push(v));
  await settle();
  return seen;
}

const answering = {
  "core.status": () => Promise.resolve(status),
  "project.list": () => Promise.resolve([project]),
};

afterEach(() => {
  vi.useRealTimers();
});

describe("connect", () => {
  it("attaches, then shows Core's status and the project list", async () => {
    const { client } = fakeClient(answering);
    const seen = await views(client);
    expect(seen[0]).toEqual({ phase: "connecting" });
    expect(seen.at(-1)).toEqual({
      phase: "connected",
      how: "launched",
      hello,
      status,
      projects: { state: "ready", projects: [project] },
    });
  });

  it("reports a Core that cannot be reached, and waits for the user", async () => {
    const error: BridgeError = { kind: "unavailable", message: "Core did not start serving" };
    const { client, attachCount } = fakeClient({}, [() => Promise.reject(error)]);
    const seen = await views(client);
    expect(seen.at(-1)).toEqual({ phase: "failed", message: "Core did not start serving" });
    expect(attachCount()).toBe(1);
  });

  it("shows Core's error when the project list fails", async () => {
    const error: BridgeError = { kind: "rpc", error: { code: -32000, message: "boom", data: { kind: "internal", detail: "disk full" } } };
    const { client } = fakeClient({
      "core.status": () => Promise.resolve(status),
      "project.list": () => Promise.reject(error),
    });
    const seen = await views(client);
    const last = seen.at(-1);
    expect(last?.phase === "connected" && last.projects).toEqual({ state: "error", message: "disk full" });
  });

  it("keeps a loss that arrives before attaching finishes", async () => {
    let finish: () => void = () => {};
    const { client, drop } = fakeClient(answering, [
      () => new Promise((r) => (finish = () => r({ how: "attached", hello }))),
      () => new Promise(() => {}),
    ]);
    const seen: CoreView[] = [];
    connect(client, (v) => seen.push(v));
    drop("Core closed the connection");
    finish();
    await settle();
    expect(seen.at(-1)).toEqual({ phase: "lost", reason: "Core closed the connection", hello });
  });

  it("marks the connection lost, and late answers do not undo that", async () => {
    let answer: (projects: Project[]) => void = () => {};
    const { client, drop } = fakeClient(
      { "core.status": () => Promise.resolve(status), "project.list": () => new Promise((r) => (answer = r)) },
      [launched, () => new Promise(() => {})],
    );
    const seen: CoreView[] = [];
    connect(client, (v) => seen.push(v));
    await settle();
    drop("Core closed the connection");
    answer([project]);
    await settle();
    expect(seen.at(-1)).toEqual({ phase: "lost", reason: "Core closed the connection", hello });
  });

  it("attaches again on its own after a loss, backing off while it fails", async () => {
    vi.useFakeTimers();
    const refused: BridgeError = { kind: "unavailable", message: "Core is restarting" };
    const { client, drop, attachCount } = fakeClient(answering, [
      launched,
      () => Promise.reject(refused),
      () => Promise.resolve({ how: "attached", hello }),
    ]);
    const seen: CoreView[] = [];
    connect(client, (v) => seen.push(v), [100, 1_000]);
    await vi.runOnlyPendingTimersAsync();
    drop("Core closed the connection");
    expect(seen.at(-1)?.phase).toBe("lost");

    // First retry after 100 ms fails; the view stays lost, with the new reason.
    await vi.advanceTimersByTimeAsync(100);
    expect(attachCount()).toBe(2);
    expect(seen.at(-1)).toEqual({ phase: "lost", reason: "Core is restarting", hello });

    // The next waits the longer delay, then succeeds.
    await vi.advanceTimersByTimeAsync(999);
    expect(attachCount()).toBe(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(attachCount()).toBe(3);
    expect(seen.at(-1)).toMatchObject({ phase: "connected", how: "attached", projects: { state: "ready" } });
  });

  it("stops retrying when stopped", async () => {
    vi.useFakeTimers();
    const { client, drop, attachCount } = fakeClient(answering);
    const stop = connect(client, () => {}, [100]);
    await vi.runOnlyPendingTimersAsync();
    drop("gone");
    stop();
    await vi.advanceTimersByTimeAsync(1_000);
    expect(attachCount()).toBe(1);
  });
});
