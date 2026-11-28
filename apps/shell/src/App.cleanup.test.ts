// @vitest-environment jsdom
// Storage's Clean up list in the window against a fake Core (FLOW-CLEAN-UP, steps 5 to 7; R16 to R19, R28, R29):
// what is listed and ticked, the one question asked before anything goes, the removals one at a time, and the states
// of a slow or failing Core. Real removals are Core's, which its own tests check.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, Method } from "./core/client";
import type { CleanupInspectResult, CleanupItem, CoreHelloResult, Session, Workspace } from "./protocol/core-api.gen";

afterEach(cleanup);

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const workspaces: Workspace[] = [{ id: "w1", project_id: "alpha", kind: "isolated", path: "D:/ws/alpha-1", vcs_ref: "ersa/fix", state: "ready" }];
const session = (id: string, title: string, archived = false): Session => ({
  id,
  title,
  workspace_id: "w1",
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  state: "idle",
  created_at: "2026-11-01T09:00:00Z",
  archived,
  pinned: false,
  unseen: false,
  tile: { state: "idle", pending: [] },
});
const listedSessions = [session("s1", "Tidy the docs"), session("s9", "Fix the build", true)];

const WT = "D:/data/worktrees/alpha";
const clean: CleanupInspectResult = { id: "ws-clean", kind: "workspace", project_id: "alpha", workspace_id: "w1", path: `${WT}/clean`, branch: "ersa/clean", session_ids: ["s9"], uncommitted: 0, ignored: 4, unshared: 2, stranded: 0, unrecorded: false, holds_repository: false, size_bytes: 3 * 1024 * 1024, token: "t-clean" };
const kept: CleanupInspectResult = { ...clean, id: "ws-kept", workspace_id: "w2", path: `${WT}/kept`, branch: "ersa/kept", session_ids: ["s1"], uncommitted: 3, stranded: 2, unshared: 0, size_bytes: 1024 * 1024, token: "t-kept" };
const repo: CleanupInspectResult = { ...clean, id: "lo-repo", kind: "leftover", workspace_id: "w3", session_ids: undefined, path: `${WT}/vendor`, branch: "ersa/vendor", unshared: 0, ignored: 0, holds_repository: true, size_bytes: 2048, token: "t-repo" };
const merged: CleanupInspectResult = { id: "br-merged", kind: "branch", project_id: "alpha", branch: "ersa/old", unshared: 1, merged: true, token: "t-merged" };
const spike: CleanupInspectResult = { id: "br-spike", kind: "branch", project_id: "alpha", branch: "ersa/spike", unshared: 4, merged: false, token: "t-spike" };
const stray: CleanupInspectResult = { id: "fo-stray", kind: "folder", project_id: "alpha", path: `${WT}/stray`, size_bytes: 1024, token: "t-stray" };
const huge: CleanupInspectResult = { id: "fo-huge", kind: "folder", project_id: "alpha", path: `${WT}/huge`, token: "t-huge" };

/** What `cleanup.list` gives for an item `cleanup.inspect` reports. */
const listed = (i: CleanupInspectResult): CleanupItem => {
  const { uncommitted: _u, ignored: _i, unshared: _s, stranded: _t, unrecorded: _r, holds_repository: _h, size_bytes: _b, merged: _m, token: _k, ...item } = i;
  return item;
};

type Answer = (params: never) => unknown;
type Held = { method: Method; params: unknown; answer: () => void; fail: (error: unknown) => void };

/**
 * A Core with one session open and a Clean up list of `items`. `hold` names the methods whose answers wait until the
 * test lets each go; `answers` replaces any method's answer.
 */
function fakeCore(items: CleanupInspectResult[], { hold = [], answers: more = {} }: { hold?: Method[]; answers?: Partial<Record<Method, Answer>> } = {}) {
  const calls: { method: Method; params: unknown }[] = [];
  const held: Held[] = [];
  let present = [...items];
  const answers: Partial<Record<Method, Answer>> = {
    "core.status": () => ({ sessions_running: 0, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => [{ id: "alpha", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true }],
    "session.list": () => listedSessions,
    "workspace.list": () => workspaces,
    "section.list": () => [],
    "backend.list": () => [],
    "settings.get": () => ({}),
    "session.subscribe": () => ({ view: "v1", last_seq: 0 }),
    "session.unsubscribe": () => ({}),
    "cleanup.list": () => ({ items: present.map(listed), unread: [] }),
    "cleanup.inspect": (params: { id: string }) => {
      const found = present.find((i) => i.id === params.id);
      if (!found) throw { kind: "rpc", error: { code: -32000, message: "cleanup.inspect: no such item", data: { kind: "not_found" } } };
      return found;
    },
    "cleanup.remove": (params: { id: string }) => {
      present = present.filter((i) => i.id !== params.id);
      return { id: params.id };
    },
    ...more,
  };
  const client: CoreClient = {
    attach: () => Promise.resolve<Attachment>({ how: "attached", hello }),
    call: (method, params) => {
      calls.push({ method, params });
      const answer = answers[method];
      if (!answer) return Promise.reject({ kind: "unavailable", message: `no answer for ${method}` }) as never;
      const run = () => answer(params as never);
      if (!hold.includes(method)) return new Promise((resolve) => resolve(run())) as never;
      return new Promise((resolve, reject) => {
        held.push({
          method,
          params,
          answer: () => {
            try {
              resolve(run());
            } catch (e) {
              reject(e);
            }
          },
          fail: reject,
        });
      }) as never;
    },
  };
  const callsOf = (method: Method) => calls.filter((c) => c.method === method).map((c) => c.params);
  const heldOf = (method: Method) => held.filter((h) => h.method === method);
  /** Lets the first held answer of `method` go. */
  const release = async (method: Method) => {
    const h = held.find((x) => x.method === method);
    if (!h) throw new Error(`nothing held for ${method}`);
    held.splice(held.indexOf(h), 1);
    await act(async () => h.answer());
    await settle();
  };
  return { client, callsOf, heldOf, release, held };
}

const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));

async function openStorage(core: ReturnType<typeof fakeCore>) {
  render(createElement(App, { client: core.client }));
  await settle();
  await settle();
  fireEvent.click(within(screen.getByRole("navigation", { name: "Rail" })).getByRole("button", { name: "Arrange sessions" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
  for (let i = 0; i < 4; i++) await settle();
}

const list = () => screen.getByRole("region", { name: "Clean up list" });
/** An item's checkbox, by the name the list shows it by. */
const box = (name: string) => within(list()).getByRole("checkbox", { name }) as HTMLInputElement;
/** All an item says beneath its name. */
const said = (name: string) => box(name).closest("label")!.textContent ?? "";
const cleanUpButton = () => within(list()).getByRole("button", { name: "Clean up…" }) as HTMLButtonElement;
const question = () => screen.queryByRole("dialog");

describe("the list", () => {
  // verifies: FLOW-CLEAN-UP-R16
  it("shows each item with what it is, its sessions and project, its size and what removing it loses, ticking exactly the lossless ones and merged branches", async () => {
    const core = fakeCore([clean, kept, merged, spike, stray]);
    await openStorage(core);
    expect(core.callsOf("cleanup.list")).toEqual([{ inactive_days: 30 }]);
    // Grouped as the brief lists them.
    expect(within(list()).getAllByRole("heading", { level: 3 }).map((h) => h.textContent)).toEqual(["Inactive and kept sessions", "Unused branches", "Folders Ersa can't identify"]);

    // A session's folder by its session's title, archived or not, with its project, branch and size.
    expect(said("Fix the build")).toContain("alpha");
    expect(said("Fix the build")).toContain("ersa/clean");
    expect(said("Fix the build")).toContain("3 MB");
    expect(said("Fix the build")).toContain("Nothing is lost.");
    expect(box("Fix the build").checked).toBe(true);
    // Ignored files and a branch's own commits are no loss; changed files and stranded commits are.
    const keptName = box("Tidy the docs");
    expect(keptName.closest("label")!.textContent).toContain("Removing it loses 3 changed files and 2 commits that exist nowhere else for good.");
    expect(keptName.checked).toBe(false);

    expect(box("ersa/old").checked).toBe(true);
    expect(said("ersa/old")).toContain("Its changes are already in the base branch.");
    expect(box("ersa/spike").checked).toBe(false);
    expect(said("ersa/spike")).toContain("4 commits that exist nowhere else");
    // Never a folder.
    expect(box("stray").checked).toBe(false);
    expect(box("stray").disabled).toBe(false);
    expect(within(list()).getByText("2 items chosen")).toBeTruthy();
  });

  // verifies: FLOW-CLEAN-UP-R28
  it("says of a folder that holds a repository of its own that it goes with the folder, and leaves it unticked", async () => {
    await openStorage(fakeCore([repo]));
    expect(within(list()).getAllByRole("heading", { level: 3 }).map((h) => h.textContent)).toEqual(["Leftover workspaces"]);
    expect(said("vendor")).toContain("Its folder holds a repository of its own, which goes with the folder and doesn't come back with it.");
    expect(box("vendor").checked).toBe(false);
  });

  // verifies: FLOW-CLEAN-UP-R29
  it("says of a folder reported without a size that Ersa couldn't finish looking through it, unticked and tickable", async () => {
    await openStorage(fakeCore([huge]));
    expect(said("huge")).toContain("Ersa couldn't finish looking through this folder, so it can't say how big it is or whether anything inside it changed.");
    expect(box("huge").checked).toBe(false);
    fireEvent.click(box("huge"));
    expect(box("huge").checked).toBe(true);
    expect(cleanUpButton().disabled).toBe(false);
  });

  // verifies: FLOW-CLEAN-UP-R29
  it("removes a folder Ersa couldn't finish looking through once it is ticked and the question confirmed, with discard", async () => {
    const core = fakeCore([huge]);
    await openStorage(core);
    fireEvent.click(box("huge"));
    fireEvent.click(cleanUpButton());
    await settle();
    fireEvent.click(within(question()!).getByRole("button", { name: "Discard and clean up" }));
    for (let i = 0; i < 3; i++) await settle();
    expect(core.callsOf("cleanup.remove")).toEqual([{ id: "fo-huge", token: "t-huge", discard: true }]);
    expect(within(list()).getByRole("status").textContent).toContain("Removed 1 of 1 item.");
  });

  // verifies: FLOW-CLEAN-UP-R19
  it("says a folder Core can't inspect can't be counted, and leaves it unticked", async () => {
    const core = fakeCore([huge], { answers: { "cleanup.inspect": () => { throw { kind: "rpc", error: { code: -32000, message: "cleanup.inspect: listing the folder's entries took longer than 30 seconds", data: { kind: "vcs_error" } } }; } } });
    await openStorage(core);
    expect(said("huge")).toContain("Ersa can't count what's in it.");
    expect(said("huge")).toContain("listing the folder's entries took longer than 30 seconds");
    expect(box("huge").checked).toBe(false);
    expect(box("huge").disabled).toBe(true);
  });

  it("is read again for the days chosen", async () => {
    const core = fakeCore([clean]);
    await openStorage(core);
    fireEvent.click(screen.getByRole("button", { name: /^Clean up inactive sessions:/ }));
    fireEvent.click(screen.getByRole("option", { name: "90 days" }));
    await settle();
    expect(core.callsOf("cleanup.list")).toEqual([{ inactive_days: 30 }, { inactive_days: 90 }]);
  });
});

describe("the question", () => {
  // verifies: FLOW-CLEAN-UP-R17
  // verifies: FLOW-CLEAN-UP-R33
  it("is asked once before anything is removed, naming how many folders and branches go, the space freed and what is lost for good", async () => {
    const core = fakeCore([clean, kept, merged, spike]);
    await openStorage(core);
    // Ticking what loses work too: the question names it, and the button reads as discarding it.
    fireEvent.click(box("Tidy the docs"));
    fireEvent.click(box("ersa/spike"));
    fireEvent.click(cleanUpButton());
    await settle();
    const asked = question()!;
    expect(within(asked).getByRole("heading").textContent).toBe("Clean up 2 folders and 2 branches?");
    expect(asked.textContent).toContain("This frees 4 MB.");
    expect(asked.textContent).toContain("Sessions whose folders go keep their conversations and branches.");
    expect(asked.textContent).toContain("You lose 3 changed files and 6 commits that exist nowhere else for good.");
    expect(within(asked).getByRole("button", { name: "Discard and clean up" })).toBeTruthy();
    expect(core.callsOf("cleanup.remove")).toEqual([]);

    // Cancel removes nothing.
    fireEvent.click(within(asked).getByRole("button", { name: "Cancel" }));
    await settle();
    expect(question()).toBeNull();
    expect(core.callsOf("cleanup.remove")).toEqual([]);
    expect(document.activeElement).toBe(cleanUpButton());

    // Unticked again, nothing is lost and the button reads as always.
    fireEvent.click(box("Tidy the docs"));
    fireEvent.click(box("ersa/spike"));
    fireEvent.click(cleanUpButton());
    await settle();
    expect(within(question()!).getByRole("heading").textContent).toBe("Clean up 1 folder and 1 branch?");
    expect(question()!.textContent).not.toContain("You lose");
    expect(within(question()!).getByRole("button", { name: "Clean up" })).toBeTruthy();
    expect(core.callsOf("cleanup.remove")).toEqual([]);
  });

  // verifies: FLOW-CLEAN-UP-R17
  it("counts a leftover's branch commits among those lost when it goes with what it holds", async () => {
    const core = fakeCore([{ ...repo, holds_repository: false, uncommitted: 2, unshared: 5 }]);
    await openStorage(core);
    expect(said("vendor")).toContain("Removing it loses 2 changed files and 5 commits that exist nowhere else for good.");
    fireEvent.click(box("vendor"));
    fireEvent.click(cleanUpButton());
    await settle();
    expect(question()!.textContent).toContain("You lose 2 changed files and 5 commits that exist nowhere else for good.");
  });

  // verifies: FLOW-CLEAN-UP-R28
  it("says how many of the folders that go hold a repository of their own", async () => {
    await openStorage(fakeCore([repo, { ...repo, id: "lo-2", path: `${WT}/vendor-2`, token: "t2" }]));
    fireEvent.click(box("vendor"));
    fireEvent.click(box("vendor-2"));
    fireEvent.click(cleanUpButton());
    await settle();
    expect(question()!.textContent).toContain("2 of the folders hold a repository of their own, which go with them.");
    expect(within(question()!).getByRole("button", { name: "Discard and clean up" })).toBeTruthy();
  });

  // verifies: FLOW-CLEAN-UP-R29
  it("says how many of the folders that go Ersa couldn't finish looking through, which the space freed leaves out", async () => {
    await openStorage(fakeCore([huge, stray]));
    fireEvent.click(box("huge"));
    fireEvent.click(box("stray"));
    fireEvent.click(cleanUpButton());
    await settle();
    expect(question()!.textContent).toContain("This frees 1 KB.");
    expect(question()!.textContent).toContain("Ersa couldn't finish looking through 1 of the folders, so the space freed leaves it out.");
  });
});

describe("cleaning up", () => {
  // verifies: FLOW-CLEAN-UP-R18
  it("removes the ticked items one at a time, saying how many are done, then says what it removed and reads the list again", async () => {
    const core = fakeCore([clean, merged, stray], { hold: ["cleanup.remove"] });
    await openStorage(core);
    fireEvent.click(box("stray"));
    fireEvent.click(cleanUpButton());
    await settle();
    fireEvent.click(within(question()!).getByRole("button", { name: "Discard and clean up" }));
    await settle();
    // One at a time: the next is sent only once the one before is answered, each with its token, and `discard` only
    // where Core needs it.
    expect(core.callsOf("cleanup.remove")).toEqual([{ id: "ws-clean", token: "t-clean" }]);
    expect(question()!.textContent).toContain("Removed 0 of 3…");
    await core.release("cleanup.remove");
    expect(core.callsOf("cleanup.remove")).toEqual([{ id: "ws-clean", token: "t-clean" }, { id: "br-merged", token: "t-merged", discard: true }]);
    expect(question()!.textContent).toContain("Removed 1 of 3…");
    await core.release("cleanup.remove");
    expect(core.callsOf("cleanup.remove")).toHaveLength(3);
    expect(core.callsOf("cleanup.remove")[2]).toEqual({ id: "fo-stray", token: "t-stray", discard: true });
    await core.release("cleanup.remove");
    expect(question()).toBeNull();
    expect(within(list()).getByRole("status").textContent).toContain("Removed 3 of 3 items.");
    expect(core.callsOf("cleanup.list")).toHaveLength(2);
    expect(within(list()).getByText("Nothing to clean up. Folders and branches Ersa no longer needs appear here.")).toBeTruthy();
    // Clean up… went with the items, so focus is on the chip just above it, not dropped to the window.
    expect(document.activeElement).toBe(screen.getByRole("button", { name: /^Clean up inactive sessions:/ }));
  });

  // verifies: FLOW-CLEAN-UP-R18
  it("stops before the next item when the user stops it", async () => {
    const core = fakeCore([clean, merged, { ...clean, id: "ws-3", path: `${WT}/three`, token: "t3" }], { hold: ["cleanup.remove"] });
    await openStorage(core);
    fireEvent.click(cleanUpButton());
    await settle();
    fireEvent.click(within(question()!).getByRole("button", { name: "Clean up" }));
    await settle();
    const stop = within(question()!).getByRole("button", { name: "Stop" });
    expect(document.activeElement).toBe(stop);
    fireEvent.click(stop);
    await settle();
    expect(within(question()!).getByRole("button", { name: "Stopping…" })).toBeTruthy();
    await core.release("cleanup.remove");
    expect(core.callsOf("cleanup.remove")).toHaveLength(1);
    expect(question()).toBeNull();
    expect(within(list()).getByRole("status").textContent).toContain("Removed 1 of 3 items. Stopped before the rest.");
    // The list is being read again, so focus is on the chip just above it.
    expect(document.activeElement).toBe(screen.getByRole("button", { name: /^Clean up inactive sessions:/ }));
  });

  // verifies: FLOW-CLEAN-UP-R18
  it("says for each item it could not remove why, shows one that changed as it is now, and goes on to the next", async () => {
    const nowKept = { ...clean, uncommitted: 2, token: "t-clean-2" };
    const core = fakeCore([clean, merged, spike], {
      hold: ["cleanup.remove"],
      answers: {
        "cleanup.remove": (params: { id: string }) => {
          if (params.id === "ws-clean") throw { kind: "rpc", error: { code: -32000, message: "cleanup.remove: ws-clean changed since it was inspected; nothing was removed", data: { kind: "invalid_state", detail: JSON.stringify(nowKept) } } };
          if (params.id === "br-merged") throw { kind: "rpc", error: { code: -32000, message: "cleanup.remove: git could not delete the branch", data: { kind: "vcs_error" } } };
          return { id: params.id };
        },
      },
    });
    await openStorage(core);
    fireEvent.click(cleanUpButton());
    await settle();
    fireEvent.click(within(question()!).getByRole("button", { name: "Clean up" }));
    await settle();
    await core.release("cleanup.remove");
    // Changed: shown as it is now, unticked since it now loses work, while the run goes on.
    expect(said("Fix the build")).toContain("Removing it loses 2 changed files for good.");
    expect(box("Fix the build").checked).toBe(false);
    expect(core.callsOf("cleanup.remove")).toHaveLength(2);
    await core.release("cleanup.remove");
    const result = within(list()).getByRole("status").textContent ?? "";
    expect(result).toContain("Removed 0 of 2 items.");
    expect(result).toContain("Couldn't remove Fix the build.");
    expect(result).toContain("It changed since it was checked, so nothing of it was removed.");
    expect(result).toContain("Couldn't remove ersa/old.");
    expect(result).toContain("git could not delete the branch");
    expect(core.callsOf("cleanup.list")).toHaveLength(2);
  });
});

describe("a slow or failing Core", () => {
  // verifies: FLOW-CLEAN-UP-R19
  it("says it is checking while the list is read and each item until it is inspected, a few at a time", async () => {
    const items = [clean, kept, merged, spike, stray];
    const core = fakeCore(items, { hold: ["cleanup.list", "cleanup.inspect"] });
    await openStorage(core);
    expect(within(list()).getByRole("status").textContent).toBe("Checking…");
    expect(within(list()).queryAllByRole("checkbox")).toEqual([]);
    await core.release("cleanup.list");
    // Three inspects at once, the rest waiting their turn.
    expect(core.heldOf("cleanup.inspect")).toHaveLength(3);
    expect(said("ersa/spike")).toContain("Checking…");
    expect(box("ersa/spike").disabled).toBe(true);
    expect(box("ersa/spike").checked).toBe(false);
    await core.release("cleanup.inspect");
    expect(core.callsOf("cleanup.inspect")).toHaveLength(4);
    expect(said("Fix the build")).toContain("Nothing is lost.");
  });

  // verifies: FLOW-CLEAN-UP-R19
  it("keeps no more than three inspects at Core when the list is read again while some are still out", async () => {
    const core = fakeCore([clean, kept, merged, spike, stray], { hold: ["cleanup.inspect"] });
    await openStorage(core);
    expect(core.heldOf("cleanup.inspect")).toHaveLength(3);
    fireEvent.click(screen.getByRole("button", { name: /^Clean up inactive sessions:/ }));
    fireEvent.click(screen.getByRole("option", { name: "60 days" }));
    await settle();
    fireEvent.click(screen.getByRole("button", { name: /^Clean up inactive sessions:/ }));
    fireEvent.click(screen.getByRole("option", { name: "90 days" }));
    await settle();
    // The older reads' inspects still hold their places: the newest read waits for them.
    expect(core.heldOf("cleanup.inspect")).toHaveLength(3);
    await core.release("cleanup.inspect");
    expect(core.heldOf("cleanup.inspect")).toHaveLength(3);
    // The answer to the older read changes nothing; the newest read's first item is what was sent next.
    expect(core.callsOf("cleanup.inspect").at(-1)).toEqual({ id: "ws-clean" });
    expect(said("Fix the build")).toContain("Checking…");
    while (core.heldOf("cleanup.inspect").length > 0) await core.release("cleanup.inspect");
    expect(said("Fix the build")).toContain("Nothing is lost.");
    expect(said("ersa/spike")).toContain("4 commits that exist nowhere else");
  });

  // verifies: FLOW-CLEAN-UP-R19
  it("says there is nothing to clean up when the list is empty", async () => {
    await openStorage(fakeCore([]));
    expect(within(list()).getByText("Nothing to clean up. Folders and branches Ersa no longer needs appear here.")).toBeTruthy();
    expect(within(list()).queryByRole("button", { name: "Clean up…" })).toBeNull();
  });

  // verifies: FLOW-CLEAN-UP-R19
  it("says why when the list can't be read, and when an item can't be inspected, leaving that item unticked", async () => {
    const failing = fakeCore([], { answers: { "cleanup.list": () => { throw { kind: "rpc", error: { code: -32000, message: "cleanup.list: the database is locked" } }; } } });
    await openStorage(failing);
    expect(list().textContent).toContain("The clean-up list can't be read.");
    expect(list().textContent).toContain("the database is locked");
    cleanup();

    const core = fakeCore([clean], { answers: { "cleanup.inspect": () => { throw { kind: "rpc", error: { code: -32000, message: "cleanup.inspect: counting the changes took longer than 60 seconds", data: { kind: "vcs_error" } } }; } } });
    await openStorage(core);
    expect(said("Fix the build")).toContain("Ersa can't count what's in it.");
    expect(said("Fix the build")).toContain("counting the changes took longer than 60 seconds");
    expect(box("Fix the build").checked).toBe(false);
    expect(box("Fix the build").disabled).toBe(true);
  });

  it("says which project's branches and folders Core couldn't look through, and why", async () => {
    const core = fakeCore([], { answers: { "cleanup.list": () => ({ items: [], unread: [{ project_id: "alpha", message: "git timed out after 10 seconds" }] }) } });
    await openStorage(core);
    expect(list().textContent).toContain("Ersa couldn't look for branches and folders in alpha.");
    expect(list().textContent).toContain("git timed out after 10 seconds");
    // Ersa didn't look there, so it doesn't say there is nothing to clean up.
    expect(list().textContent).not.toContain("Nothing to clean up.");
  });
});
