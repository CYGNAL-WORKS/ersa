// @vitest-environment jsdom
// The window's wiring for Delete (FLOW-ARCHIVE-DELETE-SESSION, steps 4 to 6): the tile's Delete… and when it is
// unavailable, the question it opens, and where focus goes once the question closes.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { Attachment, CoreClient, Method } from "./core/client";
import type { ShellControl } from "./core/shell";
import type { CoreHelloResult, Session, Workspace, WorkspaceChanges } from "./protocol/core-api.gen";

afterEach(cleanup);

const hello: CoreHelloResult = {
  core_version: "0.1.0",
  api_version: 1,
  event_projection_version: 1,
  host: { id: "local", name: "devbox", platform: "windows" },
  capabilities: [],
};

const workspaces: Workspace[] = [
  { id: "w-root", project_id: "alpha", kind: "root", path: "D:/code/alpha", vcs_ref: "main", state: "ready" },
  { id: "w-fix", project_id: "alpha", kind: "isolated", path: "D:/ws/alpha-1", vcs_ref: "ersa/fix", state: "ready" },
];

const session = (id: string, title: string, created_at: string, tile: Session["tile"], workspace_id = "w-fix"): Session => ({
  id,
  title,
  workspace_id,
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  state: tile.state,
  created_at,
  archived: false,
  pinned: false,
  unseen: false,
  tile,
});

// Newest first in the rail: s3, s2, s1.
const listed = [
  session("s1", "Fix the build", "2026-11-01T09:00:00Z", { state: "idle", pending: [] }),
  session("s2", "Write the notes", "2026-11-02T09:00:00Z", { state: "running", pending: [] }),
  session("s3", "Tidy the docs", "2026-11-03T09:00:00Z", { state: "ended", pending: [] }, "w-root"),
];

const changes: WorkspaceChanges = { uncommitted: 2, ignored: 0, unshared: 0, stranded: 0, sessions: 1, unrecorded: false, token: "t1" };

function fakeCore(more: Partial<Record<Method, (params: never) => unknown>> = {}) {
  let list = listed;
  const calls: { method: Method; params: unknown }[] = [];
  const answers: Partial<Record<Method, (params: never) => unknown>> = {
    "core.status": () => ({ sessions_running: 1, sessions_waiting: 0, uptime_s: 1, db_path: "x" }),
    "project.list": () => [{ id: "alpha", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true }],
    "session.list": () => list,
    "workspace.list": () => workspaces,
    "backend.list": () => [],
    "settings.get": () => ({}),
    "session.subscribe": () => ({ view: "v1", last_seq: 0 }),
    "session.unsubscribe": () => ({}),
    "workspace.changes": () => changes,
    "session.delete": (params: { id: string }) => {
      list = list.filter((s) => s.id !== params.id);
      return { id: params.id, workspace_deleted: true, branch_kept: false };
    },
    ...more,
  };
  const client: CoreClient = {
    attach: () => Promise.resolve<Attachment>({ how: "attached", hello }),
    call: (method, params) => {
      calls.push({ method, params });
      const answer = answers[method];
      if (!answer) return Promise.reject({ kind: "unavailable", message: `no answer for ${method}` }) as never;
      return new Promise((resolve) => resolve(answer(params as never))) as never;
    },
  };
  return { client, calls };
}

const settle = () => act(() => new Promise((r) => setTimeout(r, 0)));
const rail = () => screen.getByRole("navigation", { name: "Rail" });
const tile = (title: string) => [...rail().querySelectorAll<HTMLElement>(".session")].find((el) => el.textContent?.includes(title));
const deleteItem = (title: string) => {
  fireEvent.click(within(rail()).getByRole("button", { name: `More for ${title}` }));
  return screen.getByRole("menuitem", { name: /^Delete…/ });
};

async function open(more: Parameters<typeof fakeCore>[0] = {}) {
  const core = fakeCore(more);
  render(createElement(App, { client: core.client }));
  await settle();
  await settle();
  return core;
}

describe("Delete on a tile", () => {
  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R05
  it("is unavailable while the session is working, saying to stop it first", async () => {
    await open();
    const item = deleteItem("Write the notes");
    expect(item.getAttribute("aria-disabled")).toBe("true");
    expect(item.textContent).toContain("Stop the session to delete it");
    fireEvent.click(item);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R05
  it("is unavailable while a message is being sent to an idle session, which Core refuses as busy", async () => {
    // Idle in the tile, yet a message is on its way: Core refuses `session.delete` with `session_busy` (PROTO-CORE-API, Errors).
    const sending = listed.map((s) => (s.id === "s1" ? { ...s, sending: "copying" as const } : s));
    await open({ "session.list": () => sending });
    const item = deleteItem("Fix the build");
    expect(item.getAttribute("aria-disabled")).toBe("true");
    expect(item.textContent).toContain("Stop the session to delete it");
    fireEvent.click(item);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R04
  it("asks with what would be lost, focus on Cancel, and Cancel deletes nothing and gives focus back to the tile", async () => {
    const core = await open();
    fireEvent.click(deleteItem("Fix the build"));
    await settle();
    const dialog = screen.getByRole("dialog", { name: "Delete Fix the build?" });
    expect(dialog.textContent).toContain("The session's folder and branch ersa/fix are deleted too.");
    expect(dialog.textContent).toContain("2 uncommitted or untracked files will be lost.");
    expect(core.calls.filter((c) => c.method === "workspace.changes").map((c) => c.params)).toEqual([{ id: "w-fix" }]);
    expect(document.activeElement?.textContent).toBe("Cancel");
    // The rest of the window can't take focus while it asks.
    expect(rail().hasAttribute("inert")).toBe(true);
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await settle();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(core.calls.some((c) => c.method === "session.delete")).toBe(false);
    expect(rail().hasAttribute("inert")).toBe(false);
    expect(document.activeElement).toBe(tile("Fix the build"));
  });

  it("deletes with the token shown, and focus goes to the next tile, else the previous", async () => {
    const core = await open();
    fireEvent.click(deleteItem("Tidy the docs"));
    await settle();
    // The project's own folder: no changes to read, and no token to carry.
    expect(core.calls.some((c) => c.method === "workspace.changes")).toBe(false);
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" }));
    await settle();
    await settle();
    expect(core.calls.filter((c) => c.method === "session.delete").map((c) => c.params)).toEqual([{ id: "s3" }]);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(tile("Tidy the docs")).toBeUndefined();
    expect(document.activeElement).toBe(tile("Write the notes"));

    // The last tile in the rail has no next one: focus goes to the previous.
    fireEvent.click(deleteItem("Fix the build"));
    await settle();
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete and lose them" }));
    await settle();
    await settle();
    expect(core.calls.filter((c) => c.method === "session.delete").map((c) => c.params)).toEqual([{ id: "s3" }, { id: "s1", token: "t1" }]);
    expect(document.activeElement).toBe(tile("Write the notes"));
  });

  it("keeps a running deletion's question until it ends, and its end closes no other question", async () => {
    let finish: () => void = () => {};
    await open({
      "session.delete": (params: { id: string }) =>
        new Promise((resolve) => (finish = () => resolve({ id: params.id, workspace_deleted: false, branch_kept: false }))),
    });
    fireEvent.click(deleteItem("Tidy the docs"));
    await settle();
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" }));
    await settle();
    // Its tile says so at once, before Core lists it again.
    expect(tile("Tidy the docs")?.textContent).toContain("Deleting");
    // While Tidy the docs is being deleted, Delete… on another tile asks nothing.
    fireEvent.click(deleteItem("Fix the build"));
    await settle();
    expect(screen.getByRole("dialog", { name: "Delete Tidy the docs?" })).toBeTruthy();
    finish();
    await settle();
    expect(screen.queryByRole("dialog")).toBeNull();
    // Asked once it ended, and a cancelled question's run says nothing more.
    fireEvent.click(deleteItem("Fix the build"));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }));
    await settle();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("moves focus past tiles folded away in a collapsed group, to New session when none is left", async () => {
    const projects = ["alpha", "beta"].map((id) => ({ id, host_id: "local", path: `D:/code/${id}`, name: id, vcs: "git", trusted: true }));
    const beta: Workspace = { id: "w-beta", project_id: "beta", kind: "isolated", path: "D:/ws/beta-1", vcs_ref: "ersa/beta", state: "ready" };
    let rows = [listed[0]!, session("s4", "In the other project", "2026-11-04T09:00:00Z", { state: "idle", pending: [] }, "w-beta")];
    await open({
      "project.list": () => projects,
      "workspace.list": () => [...workspaces, beta],
      "settings.set": () => ({}),
      "session.list": () => rows,
      "session.delete": (params: { id: string }) => {
        rows = rows.filter((s) => s.id !== params.id);
        return { id: params.id, workspace_deleted: true, branch_kept: false };
      },
    });
    fireEvent.click(within(rail()).getByRole("button", { name: /^beta/ }));
    fireEvent.click(deleteItem("Fix the build"));
    await settle();
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete and lose them" }));
    await settle();
    await settle();
    expect(document.activeElement?.classList.contains("rail-new-session")).toBe(true);
  });

  // The rail reads Fix the build (alpha), In beta (beta, collapsed) and In gamma (gamma). Deleting and archiving give
  // focus to the same tile, past the one folded away; pinning keeps it on the tile, now in Pinned.
  it.each([
    ["deleted", "In gamma"],
    ["archived", "In gamma"],
    ["pinned", "Fix the build"],
  ] as const)("gives focus, as Fix the build is %s, to %s", async (how, to) => {
    const projects = ["alpha", "beta", "gamma"].map((id) => ({ id, host_id: "local", path: `D:/code/${id}`, name: id, vcs: "git", trusted: true }));
    const more = ["beta", "gamma"].map((id): Workspace => ({ id: `w-${id}`, project_id: id, kind: "isolated", path: `D:/ws/${id}-1`, vcs_ref: `ersa/${id}`, state: "ready" }));
    let rows = [
      listed[0]!,
      session("s4", "In beta", "2026-11-04T09:00:00Z", { state: "idle", pending: [] }, "w-beta"),
      session("s5", "In gamma", "2026-11-05T09:00:00Z", { state: "idle", pending: [] }, "w-gamma"),
    ];
    const change = (id: string, patch: Partial<Session>) => {
      rows = rows.map((s) => (s.id === id ? { ...s, ...patch } : s));
      return rows.find((s) => s.id === id);
    };
    await open({
      "project.list": () => projects,
      "workspace.list": () => [...workspaces, ...more],
      "settings.set": () => ({}),
      "session.list": () => rows,
      "session.delete": (params: { id: string }) => {
        rows = rows.filter((s) => s.id !== params.id);
        return { id: params.id, workspace_deleted: true, branch_kept: false };
      },
      "session.set_archived": (params: { id: string; archived: boolean }) => change(params.id, { archived: params.archived }),
      "session.arrange": (params: { id: string; pinned: boolean }) => change(params.id, { pinned: params.pinned }),
    });
    fireEvent.click(within(rail()).getByRole("button", { name: /^beta/ }));
    const menuButton = within(rail()).getByRole("button", { name: "More for Fix the build" });
    menuButton.focus();
    fireEvent.click(menuButton);
    const item = { deleted: /^Delete…/, archived: /^Archive/, pinned: /^Pin/ }[how];
    fireEvent.click(screen.getByRole("menuitem", { name: item }));
    await settle();
    if (how === "deleted") {
      fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete and lose them" }));
      await settle();
    }
    await settle();
    await settle();
    if (how === "pinned") expect(tile(to)?.closest(".rail-pinned")).toBeTruthy();
    // Within that tile: the tile itself, or the More its menu gave focus back to.
    expect(tile(to)?.closest("[data-session-id]")?.contains(document.activeElement)).toBe(true);
    expect(document.activeElement).not.toBe(document.body);
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R07
  it("shows a failed deletion on its tile, whose menu holds Delete… alone, which asks again with what is left", async () => {
    const core = await open({
      "session.list": () => listed.map((s) => (s.id === "s1" ? { ...s, deleting: "failed" as const } : s)),
    });
    expect(tile("Fix the build")?.textContent).toContain("Couldn't delete");
    const item = deleteItem("Fix the build");
    expect(screen.getAllByRole("menuitem").map((m) => m.textContent)).toEqual(["Delete…"]);
    fireEvent.click(item);
    await settle();
    expect(screen.getByRole("dialog", { name: "Delete Fix the build?" })).toBeTruthy();
    expect(core.calls.filter((c) => c.method === "workspace.changes")).toHaveLength(1);
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R07
  it("keeps the question open with the reason and Try again when the deletion fails", async () => {
    // Core marks the session's delete failed before it answers, and lists it so from then on.
    let failed = false;
    await open({
      "session.list": () => (failed ? listed.map((s) => (s.id === "s1" ? { ...s, deleting: "failed" as const } : s)) : listed),
      "session.delete": () => {
        failed = true;
        throw { kind: "rpc", error: { code: -32000, message: "session.delete: x", data: { kind: "vcs_error", detail: "The folder is in use." } } };
      },
    });
    fireEvent.click(deleteItem("Fix the build"));
    await settle();
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete and lose them" }));
    await settle();
    const dialog = screen.getByRole("dialog");
    expect(dialog.textContent).toContain("The folder is in use.");
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Try again" }));
    // The rail reads the session again: it stays, saying its deletion failed.
    await settle();
    expect(tile("Fix the build")?.textContent).toContain("Couldn't delete");
  });
});

// A session whose workspace Core put away after the window read it (FLOW-ARCHIVE-DELETE-SESSION, step 4; FLOW-CLEAN-UP).
describe("Delete on a tile whose workspace is put away", () => {
  // The window reads the workspace while it is ready; Core puts it away when the session is archived, and lists it so
  // from then on.
  const putAwayLater = () => {
    let reads = 0;
    return () => (reads++ === 0 ? workspaces : workspaces.map((w) => (w.id === "w-fix" ? { ...w, state: "put_away" } : w)));
  };

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R13
  it("says the folder is already removed and the branch goes, and deletes with nothing lost", async () => {
    const core = await open({
      "workspace.list": putAwayLater(),
      "workspace.changes": () => ({ ...changes, uncommitted: 0, token: "t-away" }),
      "session.delete": (params: { id: string }) => ({ id: params.id, workspace_deleted: false, branch_kept: false }),
    });
    fireEvent.click(deleteItem("Fix the build"));
    await settle();
    const dialog = screen.getByRole("dialog", { name: "Delete Fix the build?" });
    expect(dialog.textContent).toContain("The session's folder was already removed, and its branch ersa/fix is deleted too.");
    expect(dialog.textContent).not.toContain("folder and branch");
    expect(dialog.textContent).not.toContain("will be lost");
    const confirm = within(dialog).getAllByRole("button").find((b) => b.classList.contains("dialog-confirm"));
    expect(confirm?.querySelector("[data-shown]")?.textContent).toBe("Delete");
    fireEvent.click(confirm as HTMLElement);
    await settle();
    expect(core.calls.filter((c) => c.method === "session.delete").map((c) => c.params)).toEqual([{ id: "s1", token: "t-away" }]);
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R13
  it("says the folder is already removed and the branch is kept when it holds commits that exist nowhere else", async () => {
    await open({ "workspace.list": putAwayLater(), "workspace.changes": () => ({ ...changes, uncommitted: 0, unshared: 1 }) });
    fireEvent.click(deleteItem("Fix the build"));
    await settle();
    const dialog = screen.getByRole("dialog", { name: "Delete Fix the build?" });
    expect(dialog.textContent).toContain("The session's folder was already removed. Its branch ersa/fix is kept.");
    expect(dialog.textContent).toContain("The branch holds commits that exist nowhere else.");
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R13
  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R14
  it("says the folder is already removed and the branch stays when another session uses the workspace", async () => {
    // The branch holds a commit of its own too, but it stays for the other session, which is what the question says.
    await open({ "workspace.list": putAwayLater(), "workspace.changes": () => ({ ...changes, uncommitted: 0, unshared: 1, sessions: 2 }) });
    fireEvent.click(deleteItem("Fix the build"));
    await settle();
    const dialog = screen.getByRole("dialog", { name: "Delete Fix the build?" });
    expect(dialog.textContent).toContain("The session's folder was already removed. Another session uses its branch ersa/fix, so the branch stays.");
    expect(dialog.textContent).not.toContain("Its files stay in");
    expect(dialog.textContent).not.toContain("exist nowhere else");
  });
});

describe("the new-session screen behind the delete question", () => {
  // verifies: FLOW-NEW-SESSION-R27
  it("takes no Ctrl+O while the delete question stands over it", async () => {
    const pickFolder = vi.fn(async () => "D:/clones/stranger");
    const shell: ShellControl = {
      onWindowBack: () => () => {},
      onQuitRequested: () => () => {},
      quitHeard: async () => {},
      show: async () => false,
      hide: async () => {},
      quit: async () => {},
      openLink: () => {},
    openDiagnosticLog: async () => {},
      pickFolder,
      pickFiles: async () => [],
      onDragDrop: () => () => {},
    };
    const core = fakeCore({ "backend.list": () => [] });
    render(createElement(App, { client: core.client, shell }));
    await settle();
    await settle();
    fireEvent.click(screen.getByRole("button", { name: "New session in alpha" }));
    await settle();
    expect(screen.getByRole("region", { name: "New session" })).toBeTruthy();
    fireEvent.click(deleteItem("Fix the build"));
    await settle();
    expect(screen.getByRole("dialog", { name: "Delete Fix the build?" })).toBeTruthy();
    fireEvent.keyDown(document, { key: "o", ctrlKey: true });
    await settle();
    expect(pickFolder).not.toHaveBeenCalled();
  });
});
