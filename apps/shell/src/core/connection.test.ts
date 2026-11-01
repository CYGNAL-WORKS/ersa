import { afterEach, describe, expect, it, vi } from "vitest";
import type { CoreHelloResult, Project, Session } from "../protocol/core-api.gen";
import type { AnyNotification, Attachment, BridgeError, CoreClient, CoreEvent, Method } from "./client";
import { type CoreView, connect } from "./connection";

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const project: Project = { id: "p1", host_id: "local", path: "D:/code/ersa", name: "ersa", vcs: "git" };

const session: Session = {
  id: "s1",
  workspace_id: "w1",
  host_id: "local",
  backend: "claude-code",
  state: "running",
  created_at: "2026-11-01T00:00:00Z",
  tile: { state: "running", pending: [] },
};

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
    notify: (notification: AnyNotification) => onEvent({ kind: "notification", ...notification }),
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
  "session.list": () => Promise.resolve([session]),
};

afterEach(() => {
  vi.useRealTimers();
});

describe("connect", () => {
  it("attaches, then shows Core's status, the project list and the session list", async () => {
    const { client } = fakeClient(answering);
    const seen = await views(client);
    expect(seen[0]).toEqual({ phase: "connecting" });
    expect(seen.at(-1)).toEqual({
      phase: "connected",
      link: 1,
      how: "launched",
      hello,
      status,
      projects: { state: "ready", projects: [project] },
      sessions: { state: "ready", sessions: [session] },
    });
  });

  it("says which endpoint it attached to when it is not the user's own", async () => {
    const { client } = fakeClient(answering, [() => Promise.resolve({ how: "attached", label: "replay", hello })]);
    const seen = await views(client);
    expect(seen.at(-1)).toMatchObject({ phase: "connected", label: "replay" });
  });

  it("keeps each session's tile current, and reads the list again for a session it lacks", async () => {
    let listed = 0;
    const created: Session = { ...session, id: "s2", state: "starting", tile: { state: "starting", pending: [] } };
    const { client, notify } = fakeClient({
      ...answering,
      "session.list": () => Promise.resolve(listed++ === 0 ? [session] : [session, created]),
    });
    const seen: CoreView[] = [];
    const forwarded: AnyNotification[] = [];
    connect(client, (v) => seen.push(v), undefined, (n) => forwarded.push(n));
    await settle();

    const waiting = { state: "waiting_user" as const, pending: [{ kind: "permission" as const, request_id: "r1" }] };
    notify({ method: "session.tile", params: { session_id: "s1", tile: waiting } });
    const last = seen.at(-1);
    expect(last?.phase === "connected" && last.sessions).toEqual({
      state: "ready",
      sessions: [{ ...session, tile: waiting }],
    });

    notify({ method: "session.tile", params: { session_id: "s2", tile: created.tile } });
    await settle();
    const after = seen.at(-1);
    expect(after?.phase === "connected" && after.sessions.state === "ready" && after.sessions.sessions.map((s) => s.id)).toEqual([
      "s1",
      "s2",
    ]);
    expect(forwarded.map((n) => n.method)).toEqual(["session.tile", "session.tile"]);
  });

  it("reads a list again when asked, keeping only the newest answer", async () => {
    const answers: ((projects: Project[]) => void)[] = [];
    const { client } = fakeClient({ ...answering, "project.list": () => new Promise((r) => answers.push(r)) });
    const seen: CoreView[] = [];
    const connection = connect(client, (v) => seen.push(v));
    await settle();
    connection.reload("projects");
    const added = { ...project, id: "p2", name: "added" };
    answers[1]?.([project, added]);
    answers[0]?.([project]);
    await settle();
    const last = seen.at(-1);
    expect(last?.phase === "connected" && last.projects).toEqual({ state: "ready", projects: [project, added] });
  });

  it("reads the session list once more, not once per tile, while a read is in flight", async () => {
    const answers: ((sessions: Session[]) => void)[] = [];
    const { client, notify } = fakeClient({ ...answering, "session.list": () => new Promise((r) => answers.push(r)) });
    connect(client, () => {});
    await settle();
    for (const id of ["s2", "s3", "s4"]) notify({ method: "session.tile", params: { session_id: id, tile: session.tile } });
    expect(answers).toHaveLength(1);
    answers[0]?.([session]);
    await settle();
    expect(answers).toHaveLength(2);
  });

  it("reads the list again when a tile arrives while a read is in flight, so the older answer does not win", async () => {
    const answers: ((sessions: Session[]) => void)[] = [];
    const { client, notify } = fakeClient({ ...answering, "session.list": () => new Promise((r) => answers.push(r)) });
    const seen: CoreView[] = [];
    const connection = connect(client, (v) => seen.push(v));
    await settle();
    answers[0]?.([session]);
    await settle();
    connection.reload("sessions");
    const waiting = { state: "waiting_user" as const, pending: [{ kind: "permission" as const, request_id: "r1" }] };
    notify({ method: "session.tile", params: { session_id: "s1", tile: waiting } });
    // The read in flight was taken before the tile: it answers with the old one.
    answers[1]?.([session]);
    await settle();
    answers[2]?.([{ ...session, tile: waiting }]);
    await settle();
    const last = seen.at(-1);
    expect(answers).toHaveLength(3);
    expect(last?.phase === "connected" && last.sessions).toEqual({ state: "ready", sessions: [{ ...session, tile: waiting }] });
  });

  it("numbers each connection, so subscriptions know to start again", async () => {
    vi.useFakeTimers();
    const { client, drop } = fakeClient(answering);
    const seen: CoreView[] = [];
    connect(client, (v) => seen.push(v), [100]);
    await vi.runOnlyPendingTimersAsync();
    expect(seen.at(-1)).toMatchObject({ phase: "connected", link: 1 });
    drop("gone");
    await vi.advanceTimersByTimeAsync(100);
    expect(seen.at(-1)).toMatchObject({ phase: "connected", link: 2 });
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
    const connection = connect(client, () => {}, [100]);
    await vi.runOnlyPendingTimersAsync();
    drop("gone");
    connection.stop();
    await vi.advanceTimersByTimeAsync(1_000);
    expect(attachCount()).toBe(1);
  });
});
