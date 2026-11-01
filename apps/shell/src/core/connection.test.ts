import { describe, expect, it } from "vitest";
import type { CoreHelloResult, Project } from "../protocol/core-api.gen";
import type { Attachment, BridgeError, CoreClient, Method } from "./client";
import { type CoreView, connect } from "./connection";

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const project: Project = { id: "p1", host_id: "local", path: "D:/code/ersa", name: "ersa", vcs: "git" };

/** A client whose answers the test sets per method, and whose connection the test can drop. */
function fakeClient(
  answers: Partial<Record<Method, () => Promise<unknown>>>,
  attach: () => Promise<Attachment> = () => Promise.resolve({ how: "launched", hello }),
) {
  const lost: ((reason: string) => void)[] = [];
  const client: CoreClient = {
    attach,
    call: (method) => (answers[method]?.() ?? Promise.reject({ kind: "unavailable", message: `no answer for ${method}` })) as never,
    onConnectionLost: (listener) => {
      lost.push(listener);
      return () => lost.splice(lost.indexOf(listener), 1);
    },
    onNotification: () => () => {},
  };
  return { client, drop: (reason: string) => lost.forEach((l) => l(reason)) };
}

/** Runs `connect` until its promises settle and returns every view it reported. */
async function views(client: CoreClient): Promise<CoreView[]> {
  const seen: CoreView[] = [];
  connect(client, (v) => seen.push(v));
  await new Promise((r) => setTimeout(r, 0));
  return seen;
}

const status = { sessions_running: 2, sessions_waiting: 1, uptime_s: 5, db_path: "x" };

describe("connect", () => {
  it("attaches, then shows Core's status and the project list", async () => {
    const { client } = fakeClient({
      "core.status": () => Promise.resolve(status),
      "project.list": () => Promise.resolve([project]),
    });
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

  it("reports a Core that cannot be reached", async () => {
    const error: BridgeError = { kind: "unavailable", message: "Core did not start serving" };
    const { client } = fakeClient({}, () => Promise.reject(error));
    const seen = await views(client);
    expect(seen.at(-1)).toEqual({ phase: "failed", message: "Core did not start serving" });
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
    const { client, drop } = fakeClient({}, () => new Promise((r) => (finish = () => r({ how: "attached", hello }))));
    const seen: CoreView[] = [];
    connect(client, (v) => seen.push(v));
    drop("Core closed the connection");
    finish();
    await new Promise((r) => setTimeout(r, 0));
    expect(seen.at(-1)).toEqual({ phase: "lost", reason: "Core closed the connection", hello });
  });

  it("marks the connection lost, and late answers do not undo that", async () => {
    let answer: (projects: Project[]) => void = () => {};
    const { client, drop } = fakeClient({
      "core.status": () => Promise.resolve(status),
      "project.list": () => new Promise((r) => (answer = r)),
    });
    const seen: CoreView[] = [];
    connect(client, (v) => seen.push(v));
    await new Promise((r) => setTimeout(r, 0));
    drop("Core closed the connection");
    answer([project]);
    await new Promise((r) => setTimeout(r, 0));
    expect(seen.at(-1)).toEqual({ phase: "lost", reason: "Core closed the connection", hello });
  });
});
