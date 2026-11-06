import { describe, expect, it } from "vitest";
import type { Project } from "../protocol/core-api.gen";
import type { Session } from "../protocol/core-api.gen";
import { type BackendChoices, type Choice, type SessionValues, projectOrder, recentFirst, seedScreen, sessionShown, showsBackend, startingProject, valuesFor, workspaceKinds } from "./controls";

const project = (vcs: Project["vcs"]): Project => ({ id: "p1", host_id: "local", path: "D:\\Projects\\p", name: "p", vcs, trusted: true });

const choices = (ids: string[], dflt: string): Choice[] => ids.map((id) => ({ id, label: id, default: id === dflt }));

const backend = (name: string, status: BackendChoices["status"] = "available"): BackendChoices => ({
  name,
  label: name,
  status,
  tested: { min: "1.0.0", max: "2.0.0" },
  models: choices([`${name}-small`, `${name}-large`], `${name}-large`),
  efforts: choices(["low", "high"], "high"),
  permission_modes: choices(["ask", "edits", "bypass"], "ask"),
});

describe("the session controls' starting values", () => {
  // verifies: FLOW-NEW-SESSION-R02
  it("start a git project with nothing last used at a new workspace and the first available backend's defaults", () => {
    const values = valuesFor(project("git"), [backend("copilot", "missing"), backend("claude-code"), backend("other")]);
    expect(values).toEqual({
      host_id: "local",
      project_id: "p1",
      workspace: "isolated",
      backend: "claude-code",
      model: "claude-code-large",
      effort: "high",
      permission_mode: "ask",
    });
  });

  // verifies: FLOW-NEW-SESSION-R02
  it("start a project without version control at the root workspace", () => {
    expect(valuesFor(project("none"), [backend("claude-code")]).workspace).toBe("root");
  });

  // verifies: FLOW-NEW-SESSION-R02
  it("start at the values last used for the project", () => {
    const last = { workspace: "root", backend: "other", model: "other-small", effort: "low", permission_mode: "edits" } as const;
    expect(valuesFor(project("git"), [backend("claude-code"), backend("other")], last)).toMatchObject(last);
  });

  // verifies: FLOW-NEW-SESSION-R02
  it("fall back to the defaults for any last-used value the host no longer offers", () => {
    const last = { workspace: "isolated", backend: "copilot", model: "copilot-small", effort: "low" } as const;
    // The last backend has gone missing, so its model and effort mean nothing to the one that replaces it.
    const values = valuesFor(project("perforce"), [backend("copilot", "error"), backend("claude-code")], last);
    expect(values).toMatchObject({ workspace: "root", backend: "claude-code", model: "claude-code-large", effort: "high" });
  });

  // verifies: FLOW-NEW-SESSION-R02
  it("take the first choice when a backend marks none as default", () => {
    const plain = { ...backend("claude-code"), efforts: choices(["low", "high"], "none") };
    expect(valuesFor(project("git"), [plain]).effort).toBe("low");
  });

  it("name no backend, model, effort or permission mode when none is available", () => {
    const values = valuesFor(project("git"), [backend("claude-code", "missing")]);
    expect(values.backend).toBeUndefined();
    expect(values.model).toBeUndefined();
    expect(values.permission_mode).toBeUndefined();
  });
});

describe("the workspace choices", () => {
  // verifies: FLOW-NEW-SESSION-R06
  it("are the root workspace only where Ersa has no workspace adapter", () => {
    expect(workspaceKinds(project("git"))).toEqual(["isolated", "root"]);
    expect(workspaceKinds(project("perforce"))).toEqual(["root"]);
    expect(workspaceKinds(project("none"))).toEqual(["root"]);
  });
});

describe("the backend chip", () => {
  it("shows when the host has more than one backend, and hides with one", () => {
    expect(showsBackend([backend("claude-code"), backend("copilot")])).toBe(true);
    expect(showsBackend([backend("claude-code")])).toBe(false);
  });

  // verifies: FLOW-NEW-SESSION-R07
  it("shows a lone backend that is missing, so its status is not hidden", () => {
    expect(showsBackend([backend("claude-code", "missing")])).toBe(true);
    expect(showsBackend([backend("claude-code", "error")])).toBe(true);
  });
});

describe("the session controls under a running session", () => {
  const running = { host_id: "local", backend: "claude", model: "claude-small", effort: "low", permission_mode: "edits" };
  const branch = { project_id: "p1", kind: "isolated" as const, vcs_ref: "ersa/fix-build" };

  // verifies: FLOW-NEW-SESSION-R12
  it("show the session's own values and its branch, not the host's defaults", () => {
    const shown = sessionShown(running, branch, [backend("claude"), backend("copilot")]);
    expect(shown.values).toEqual({
      host_id: "local",
      project_id: "p1",
      workspace: "isolated",
      backend: "claude",
      model: "claude-small",
      effort: "low",
      permission_mode: "edits",
    });
    expect(shown.workspaceLabel).toBe("ersa/fix-build");
  });

  // verifies: FLOW-NEW-SESSION-R12
  it("keep the session's values when the host lists no backend, or no longer offers them", () => {
    const none = sessionShown(running, branch, []);
    expect(none.values.backend).toBe("claude");
    expect(none.values.model).toBe("claude-small");
    expect(none.backend.models.map((c) => c.id)).toEqual(["claude-small"]);
    const changed = sessionShown({ ...running, model: "retired" }, branch, [backend("claude")]);
    expect(changed.values.model).toBe("retired");
    expect(changed.backend.models.map((c) => c.id)).toContain("retired");
  });

  it("show a value left to the backend as the backend's default, and the root workspace by name", () => {
    const shown = sessionShown({ ...running, model: undefined }, { ...branch, kind: "root", vcs_ref: "" }, [backend("claude")]);
    expect(shown.values.model).toBe("claude-large");
    expect(shown.workspaceLabel).toBe("Root workspace");
  });
});

describe("the project list's order", () => {
  const named = (id: string, host_id = "local"): Project => ({ ...project("git"), id, host_id, name: id });
  const workspaces = { w1: { project_id: "a" }, w2: { project_id: "b" }, w3: { project_id: "b" }, w4: { project_id: "c" } };
  const session = (workspace_id: string, created_at: string) => ({ workspace_id, created_at });

  // verifies: FLOW-NEW-SESSION-R14
  it("puts the project whose newest session is newest first", () => {
    const sessions = [
      session("w1", "2026-11-01T09:00:00Z"),
      // b's older session is older than a's, but its newest is newer than a's: the newest session decides.
      session("w2", "2026-10-01T09:00:00Z"),
      session("w3", "2026-11-03T09:00:00Z"),
      session("w4", "2026-11-02T09:00:00Z"),
    ];
    expect(recentFirst([named("a"), named("b"), named("c")], sessions, workspaces).map((p) => p.id)).toEqual(["b", "c", "a"]);
  });

  // verifies: FLOW-NEW-SESSION-R14
  it("puts projects with no session after the used ones, in the order Core lists them", () => {
    const sessions = [session("w4", "2026-11-02T09:00:00Z"), session("unknown-workspace", "2026-11-05T09:00:00Z")];
    expect(recentFirst([named("a"), named("b"), named("c"), named("d")], sessions, workspaces).map((p) => p.id)).toEqual(["c", "a", "b", "d"]);
  });

  // verifies: FLOW-NEW-SESSION-R15
  it("starts the screen opened for no particular project at the local host's most recently used project", () => {
    const ordered = [named("remote", "far"), named("b"), named("a")];
    expect(startingProject(ordered, "local")?.id).toBe("b");
    expect(startingProject(ordered, "local", "a")?.id).toBe("a");
    // A project that is gone, or a host with no project of its own: the most recently used of all.
    expect(startingProject(ordered, "local", "gone")?.id).toBe("b");
    expect(startingProject(ordered, "nowhere")?.id).toBe("remote");
    expect(startingProject([], "local")).toBeUndefined();
  });
});

describe("the project list's order as the window keeps it", () => {
  const named = (id: string): Project => ({ ...project("git"), id, name: id });
  const workspaces = { w1: { project_id: "a" }, w2: { project_id: "b" } };
  const session = (workspace_id: string, created_at: string) => ({ workspace_id, created_at }) as Session;
  const projects = [named("a"), named("b"), named("c")];

  // verifies: FLOW-NEW-SESSION-R14
  it("orders by the sessions Core lists, and keeps them", () => {
    const listed = [session("w1", "2026-11-01T09:00:00Z"), session("w2", "2026-11-02T09:00:00Z")];
    const order = projectOrder(projects, listed, [], workspaces);
    expect(order.projects.map((p) => p.id)).toEqual(["b", "a", "c"]);
    expect(order.kept).toBe(listed);
  });

  // verifies: FLOW-NEW-SESSION-R14
  it("keeps the last order while the session list reloads, rather than falling back to Core's", () => {
    const kept = [session("w2", "2026-11-02T09:00:00Z")];
    const order = projectOrder(projects, undefined, kept, workspaces);
    expect(order.projects.map((p) => p.id)).toEqual(["b", "a", "c"]);
    expect(order.kept).toBe(kept);
  });
});

describe("where the new-session screen starts", () => {
  const named = (id: string): Project => ({ ...project("git"), id, name: id });
  const used = (project_id: string): SessionValues => ({ host_id: "local", project_id, workspace: "root" });

  // verifies: FLOW-NEW-SESSION-R15
  it("moves to the most recently used project when the order arrives after the screen opened", () => {
    // At launch the screen opens on Core's first project; the sessions and workspaces arrive, and b comes first.
    const opened = seedScreen(undefined, named("a"), undefined, {});
    expect(opened.wanted.project_id).toBe("a");
    // No last-used values change meanwhile: the starting project alone moves the screen.
    const later = seedScreen(opened, named("b"), undefined, {});
    expect(later.wanted).toEqual({ project_id: "b" });
    // Nothing changed after that: the same seed, so the screen does not render again.
    expect(seedScreen(later, named("b"), undefined, {})).toBe(later);
  });

  // verifies: FLOW-NEW-SESSION-R15
  it("stays where the user saw it once they chose, wrote or sent", () => {
    const touched = { ...seedScreen(undefined, named("a"), undefined, {}), touched: true };
    expect(seedScreen(touched, named("b"), undefined, {})).toBe(touched);
  });

  // verifies: FLOW-NEW-SESSION-R02
  it("starts afresh when New session is chosen for another project, even after the user chose", () => {
    const touched = { ...seedScreen(undefined, named("a"), undefined, {}), touched: true };
    const next = seedScreen(touched, named("c"), "c", { c: used("c") });
    expect(next).toEqual({ shownFor: "c", seededWith: used("c"), touched: false, wanted: { ...used("c"), project_id: "c" } });
  });
});
