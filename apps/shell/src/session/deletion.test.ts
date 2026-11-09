import { describe, expect, it } from "vitest";
import type { BridgeError, CoreClient, Method, Params } from "../core/client";
import type { Session, Workspace, WorkspaceChanges } from "../protocol/core-api.gen";
import { type DeletePhase, Deletion, deleteUnavailable, losesWork, whatGoes } from "./deletion";

/** A client that records each call and answers from `answers`, in turn when an answer is a list. */
function fakeCore(answers: Partial<Record<Method, ((params: never) => unknown)[]>>) {
  const calls: { method: Method; params: unknown }[] = [];
  const client: Pick<CoreClient, "call"> = {
    call: <M extends Method>(method: M, params?: Params<M>) => {
      calls.push({ method, params });
      const answer = answers[method]?.shift();
      if (!answer) return Promise.reject(new Error(`unexpected ${method}`)) as never;
      try {
        return Promise.resolve(answer(params as never)) as never;
      } catch (error) {
        return Promise.reject(error) as never;
      }
    },
  };
  return { client, calls };
}

const refusal = (kind: string, detail?: string): BridgeError => ({
  kind: "rpc",
  error: { code: -32000, message: "session.delete: refused", data: { kind, detail } } as never,
});

const isolated: Workspace = { id: "w1", project_id: "p1", kind: "isolated", path: "D:\\ws\\ersa-1", vcs_ref: "ersa/fix-build", state: "ready" };
const root: Workspace = { id: "w0", project_id: "p1", kind: "root", path: "D:\\Projects\\ersa", vcs_ref: "", state: "ready" };
const clean: WorkspaceChanges = { uncommitted: 0, ignored: 0, unshared: 0, stranded: 0, sessions: 1, unrecorded: false, token: "t1" };

function run(client: Pick<CoreClient, "call">, workspace = isolated) {
  const phases: DeletePhase[] = [];
  const deletion = new Deletion(client, "s1", workspace, (p) => phases.push(p));
  return { deletion, phases, last: () => phases[phases.length - 1] };
}

describe("what the delete question says goes", () => {
  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R04
  it("names the branch, says it goes, and gives the counts lost with the folder", () => {
    const goes = whatGoes(isolated, { ...clean, uncommitted: 3, ignored: 12, unshared: 2, stranded: 2 });
    expect(goes).toEqual({ kind: "folderGoes", branch: "ersa/fix-build", branchKept: false, uncommitted: 3, ignored: 12, stranded: 2, unrecorded: false });
    expect(losesWork(goes)).toBe(true);
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R04
  it("keeps the branch when it holds commits no other branch or remote holds", () => {
    // Five unshared commits, two of them off the branch: the branch holds three, so it stays.
    const goes = whatGoes(isolated, { ...clean, unshared: 5, stranded: 2 });
    expect(goes).toMatchObject({ kind: "folderGoes", branchKept: true, stranded: 2 });
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R04
  it("loses nothing from a clean workspace", () => {
    const goes = whatGoes(isolated, clean);
    expect(goes).toMatchObject({ kind: "folderGoes", branchKept: false, uncommitted: 0, ignored: 0, stranded: 0 });
    expect(losesWork(goes)).toBe(false);
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R04
  it("says the files stay, and where, for the root workspace and for one another session uses", () => {
    expect(whatGoes(root, undefined)).toEqual({ kind: "filesStay", path: "D:\\Projects\\ersa" });
    expect(whatGoes(isolated, { ...clean, uncommitted: 4, sessions: 2 })).toEqual({ kind: "filesStay", path: "D:\\ws\\ersa-1" });
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R04
  it("counts the other sessions alone once this one is marked deleting, which Core leaves out of the count", () => {
    // Asked again after a failed delete: one other session still uses the folder, so its files stay.
    expect(whatGoes(isolated, { ...clean, sessions: 1 }, true)).toEqual({ kind: "filesStay", path: "D:\\ws\\ersa-1" });
    expect(whatGoes(isolated, { ...clean, sessions: 0 }, true)).toMatchObject({ kind: "folderGoes" });
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R10
  it("says commits on a detached checkout can't be counted when git has lost the workspace's record", () => {
    const goes = whatGoes(isolated, { ...clean, unshared: 1, unrecorded: true });
    expect(goes).toMatchObject({ kind: "folderGoes", unrecorded: true, stranded: 0, branchKept: true });
    expect(losesWork(goes)).toBe(true);
  });
});

describe("when Delete is unavailable", () => {
  const session = (state: string, pending: Session["tile"]["pending"] = []) =>
    ({ id: "s1", tile: { state, pending } }) as unknown as Session;

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R05
  it("says to stop the session first while it is starting, working or waiting on the user", () => {
    for (const state of ["starting", "running", "waiting_user"]) expect(deleteUnavailable(session(state), false)).toBe("Stop the session to delete it");
    expect(deleteUnavailable(session("idle", [{ kind: "permission", request_id: "r1" }]), false)).toBe("Stop the session to delete it");
    for (const state of ["idle", "ended", "error", "disconnected"]) expect(deleteUnavailable(session(state), false)).toBeUndefined();
  });

  it("says Ersa is deleting it while a deletion runs, and offers it again once one failed", () => {
    expect(deleteUnavailable({ ...session("idle"), deleting: "running" }, false)).toBe("Ersa is deleting this session");
    expect(deleteUnavailable({ ...session("idle"), deleting: "failed" }, false)).toBeUndefined();
  });

  it("says why while Ersa can't reach Core", () => {
    expect(deleteUnavailable(session("idle"), true)).toBe("Available once Ersa reaches your sessions");
  });
});

describe("the delete question against Core", () => {
  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R04
  it("asks with what would be lost, then deletes with the token of what was shown", async () => {
    const { client, calls } = fakeCore({
      "workspace.changes": [() => ({ ...clean, uncommitted: 2, token: "shown" })],
      "session.delete": [() => ({ id: "s1", workspace_deleted: true, branch_kept: false })],
    });
    const { deletion, phases, last } = run(client);
    await deletion.start();
    expect(phases[0]).toEqual({ kind: "reading" });
    expect(last()).toMatchObject({ kind: "asking", changed: false, goes: { uncommitted: 2 } });
    await deletion.confirm();
    expect(calls.filter((c) => c.method !== "session.list").map((c) => [c.method, c.params])).toEqual([
      ["workspace.changes", { id: "w1" }],
      ["session.delete", { id: "s1", token: "shown" }],
    ]);
    expect(phases.slice(-2).map((p) => p.kind)).toEqual(["deleting", "deleted"]);
  });

  it("asks without reading changes for the root workspace, and deletes without a token", async () => {
    const { client, calls } = fakeCore({ "session.delete": [() => ({ id: "s1", workspace_deleted: false, branch_kept: false })] });
    const { deletion, last } = run(client, root);
    await deletion.start();
    expect(last()).toEqual({ kind: "asking", changed: false, goes: { kind: "filesStay", path: "D:\\Projects\\ersa" } });
    await deletion.confirm();
    expect(calls).toEqual([{ method: "session.delete", params: { id: "s1" } }]);
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R08
  it("asks again with what Core now counts when the token is stale, and deletes with the new one", async () => {
    const now = { ...clean, uncommitted: 3, token: "now" };
    const { client, calls } = fakeCore({
      "workspace.changes": [() => clean],
      "session.delete": [
        () => {
          throw refusal("invalid_state", JSON.stringify(now));
        },
        () => ({ id: "s1", workspace_deleted: true, branch_kept: false }),
      ],
    });
    const { deletion, last } = run(client);
    await deletion.start();
    await deletion.confirm();
    // Asked again, not deleted: the question now counts the three files, and says it changed.
    expect(last()).toMatchObject({ kind: "asking", changed: true, goes: { uncommitted: 3 } });
    await deletion.confirm();
    expect(calls.filter((c) => c.method !== "session.list").map((c) => c.params)).toEqual([{ id: "w1" }, { id: "s1", token: "t1" }, { id: "s1", token: "now" }]);
    expect(last()).toEqual({ kind: "deleted" });
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R08
  it("asks again even when the counts are the same and only the token changed", async () => {
    const { client } = fakeCore({
      "workspace.changes": [() => clean],
      "session.delete": [
        () => {
          throw refusal("invalid_state", JSON.stringify({ ...clean, token: "t2" }));
        },
      ],
    });
    const { deletion, last } = run(client);
    await deletion.start();
    await deletion.confirm();
    expect(last()).toMatchObject({ kind: "asking", changed: true });
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R07
  it("says what failed and, on Try again, asks again with what is left before deleting again", async () => {
    const { client, calls } = fakeCore({
      "workspace.changes": [() => ({ ...clean, uncommitted: 5 }), () => ({ ...clean, uncommitted: 1, token: "left" })],
      "session.delete": [
        () => {
          throw refusal("vcs_error", "The folder is in use by another program.");
        },
        () => ({ id: "s1", workspace_deleted: true, branch_kept: false }),
      ],
    });
    const { deletion, last } = run(client);
    await deletion.start();
    await deletion.confirm();
    expect(last()).toEqual({ kind: "failed", reason: "The folder is in use by another program." });
    // Try again reads what is left and asks; nothing is deleted until the user confirms again.
    await deletion.start();
    expect(last()).toMatchObject({ kind: "asking", changed: false, goes: { uncommitted: 1 } });
    expect(calls.filter((c) => c.method === "session.delete")).toHaveLength(1);
    await deletion.confirm();
    expect(calls[calls.length - 1]?.params).toEqual({ id: "s1", token: "left" });
    expect(last()).toEqual({ kind: "deleted" });
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R07
  it("on Try again, says the files stay when another session shares the folder of a session marked deleting", async () => {
    const { client } = fakeCore({
      "session.list": [() => [{ id: "s1" }], () => [{ id: "s1", deleting: "failed" }, { id: "s2" }]],
      "workspace.changes": [() => ({ ...clean, sessions: 2 }), () => ({ ...clean, uncommitted: 3, sessions: 1, token: "left" })],
      "session.delete": [
        () => {
          throw refusal("vcs_error", "The backend could not delete the record.");
        },
      ],
    });
    const { deletion, last } = run(client);
    await deletion.start();
    expect(last()).toMatchObject({ kind: "asking", goes: { kind: "filesStay" } });
    await deletion.confirm();
    await deletion.start();
    expect(last()).toEqual({ kind: "asking", changed: false, goes: { kind: "filesStay", path: "D:\\ws\\ersa-1" } });
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R07
  it("treats a refusal whose detail is not a count of changes as a failure", async () => {
    const { client } = fakeCore({
      "workspace.changes": [() => clean],
      "session.delete": [
        () => {
          throw refusal("invalid_state", "The session is open in claude (process 4120).");
        },
      ],
    });
    const { deletion, last } = run(client);
    await deletion.start();
    await deletion.confirm();
    expect(last()).toEqual({ kind: "failed", reason: "The session is open in claude (process 4120)." });
  });

  it("says in a sentence why when Core answers that something else holds the session", async () => {
    const { client } = fakeCore({
      "workspace.changes": [() => clean],
      "session.delete": [
        () => {
          throw refusal("invalid_state", "busy");
        },
      ],
    });
    const { deletion, last } = run(client);
    await deletion.start();
    await deletion.confirm();
    expect(last()).toEqual({ kind: "failed", reason: "Something else is changing this session or its folder. Try again in a moment." });
  });

  it("fails with the reason when what would be lost can't be read", async () => {
    const { client } = fakeCore({
      "workspace.changes": [
        () => {
          throw refusal("vcs_error", "git took too long to answer.");
        },
      ],
    });
    const { deletion, last } = run(client);
    await deletion.start();
    expect(last()).toEqual({ kind: "failed", reason: "git took too long to answer." });
  });
});
