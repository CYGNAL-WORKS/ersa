import { describe, expect, it } from "vitest";
import type { BackendInfo, Choice, Project } from "../protocol/core-api.gen";
import type { Session } from "../protocol/core-api.gen";
import { type SessionValues, effortsFor, projectOrder, recentFirst, seedScreen, sessionShown, startingProject, valuesFor, workspaceKinds } from "./controls";

const project = (vcs: Project["vcs"]): Project => ({ id: "p1", host_id: "local", path: "D:\\Projects\\p", name: "p", vcs, trusted: true });

const choices = (ids: string[], dflt: string): Choice[] => ids.map((id) => ({ id, label: id, default: id === dflt }));

const backend = (name: string, status: BackendInfo["status"] = "available"): BackendInfo => ({
  name,
  label: name,
  status,
  tested: { min: "1.0.0", max: "2.0.0" },
  models: choices([`${name}-small`, `${name}-large`], `${name}-large`).map((m) => ({ ...m, efforts: ["low", "high"] })),
  efforts: choices(["low", "high"], "high"),
  permission_modes: choices(["manual", "edits", "bypass"], "manual"),
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
      permission_mode: "manual",
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
  it("take the first model when a backend marks none as default, and no effort when it marks no effort", () => {
    const plain = { ...backend("claude-code"), models: choices(["a", "b"], "none"), efforts: choices(["low", "high"], "none") };
    expect(valuesFor(project("git"), [plain])).toMatchObject({ model: "a", effort: undefined });
  });

  /** Claude Code as `backend.list` lists it: the CLI's own `default` entry first, a model without efforts, and no effort marked. */
  const claude: BackendInfo = {
    ...backend("claude-code"),
    models: [
      { id: "default", label: "Default (recommended)", default: true, resolved: "claude-opus-5-5", efforts: ["low", "high"] },
      { id: "opus", label: "Opus 5.5", default: false, resolved: "claude-opus-5-5", efforts: ["low", "high"] },
      { id: "claude-haiku-4-5", label: "Haiku 4.5", default: false, resolved: "claude-haiku-4-5" },
      { id: "older", label: "Older", default: false, efforts: ["low"] },
    ],
    efforts: choices(["low", "high"], "none"),
  };

  // verifies: FLOW-NEW-SESSION-R02
  it("start at the backend's own default model and no effort", () => {
    expect(valuesFor(project("git"), [claude])).toMatchObject({ model: "default", effort: undefined });
  });

  // verifies: FLOW-NEW-SESSION-R02
  it("keep an effort only while the chosen model takes it", () => {
    const wanted = { backend: "claude-code", effort: "high" };
    expect(valuesFor(project("git"), [claude], { ...wanted, model: "opus" }).effort).toBe("high");
    expect(valuesFor(project("git"), [claude], { ...wanted, model: "older" }).effort).toBeUndefined();
    expect(valuesFor(project("git"), [claude], { ...wanted, model: "claude-haiku-4-5" }).effort).toBeUndefined();
  });

  it("offer each model only the efforts it takes", () => {
    expect(effortsFor(claude, "older").map((e) => e.id)).toEqual(["low"]);
    expect(effortsFor(claude, "claude-haiku-4-5")).toEqual([]);
    // A model the backend does not list, as a session's own, is offered every level.
    expect(effortsFor(claude, "retired").map((e) => e.id)).toEqual(["low", "high"]);
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

describe("the session controls under a running session", () => {
  const running = { host_id: "local", backend: "claude", model: "claude-small", effort: "low", permission_mode: "edits" };
  const branch = { project_id: "p1", kind: "isolated" as const, vcs_ref: "ersa/fix-build" };

  // verifies: FLOW-NEW-SESSION-R12
  it("show the session's own values, not the host's defaults", () => {
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

  // The CLI's own list, as `backend.list` gives it: two aliases and a full id, Default resolving to the full id.
  const claude: BackendInfo = {
    ...backend("claude"),
    models: [
      { id: "default", label: "Default (recommended)", default: true, resolved: "claude-fable-5-2", efforts: ["low", "high"] },
      { id: "opus", label: "Opus 5.5", default: false, resolved: "claude-opus-5-5", efforts: ["low", "high"] },
      { id: "claude-fable-5-2", label: "Fable 5.2", default: false, resolved: "claude-fable-5-2", efforts: ["low", "high"] },
      { id: "haiku", label: "Haiku", default: false, resolved: "claude-haiku-5-5" },
    ],
  };

  // verifies: FLOW-NEW-SESSION-R33
  it("name the model by the choice requested while the backend reports what it resolves to, Default and an alias alike", () => {
    // Default and `opus` both resolve to `claude-opus-5-5`, as Claude Code's own list can have them.
    const both: BackendInfo = { ...claude, models: claude.models.map((m) => (m.id === "default" ? { ...m, resolved: "claude-opus-5-5" } : m)) };
    const named = (model: string | undefined, model_request: string | undefined, host = both) =>
      sessionShown({ ...running, model, model_request }, branch, [host]).values.model;
    // Claude Code reports `claude-opus-5-5` once a turn has run with `--model opus`, or with Default.
    expect(named("claude-opus-5-5", "opus")).toBe("opus");
    expect(named("claude-opus-5-5", "default")).toBe("default");
    // Default resolving to an id another choice is listed under: still Default, since the request comes first.
    expect(named("claude-fable-5-2", "default", claude)).toBe("default");
    expect(named("claude-fable-5-2", undefined, claude)).toBe("default");
    // Before the backend reports one: the request, else the backend's default.
    expect(named(undefined, "opus")).toBe("opus");
    expect(named(undefined, undefined)).toBe("default");
    // A request the host no longer lists still names it, under its id.
    expect(named(undefined, "retired")).toBe("retired");
    // No request: the backend's default names what it resolves to.
    expect(named("claude-opus-5-5", undefined)).toBe("default");
    // The session's request with `opus` reported as itself, before the stream resolves it.
    expect(named("opus", "opus")).toBe("opus");
  });

  // verifies: FLOW-NEW-SESSION-R33
  it("name a model the session changed to since its request by the report", () => {
    const named = (model: string, model_request: string) => sessionShown({ ...running, model, model_request }, branch, [claude]).values.model;
    // Listed under its own id: that choice, though Default resolves to it too.
    expect(named("claude-fable-5-2", "opus")).toBe("claude-fable-5-2");
    // Not listed by id: the choice not marked default that resolves to it.
    expect(named("claude-haiku-5-5", "opus")).toBe("haiku");
    // Only Default resolves to it: the id itself, since the session left the request and Default for a model.
    const onlyDefault = { ...claude, models: claude.models.filter((m) => m.id !== "claude-fable-5-2") };
    expect(sessionShown({ ...running, model: "claude-fable-5-2", model_request: "opus" }, branch, [onlyDefault]).values.model).toBe("claude-fable-5-2");
    // A model that takes no effort keeps taking none under its alias; one under an alias takes the alias's.
    expect(effortsFor(claude, named("claude-haiku-5-5", "opus"))).toEqual([]);
    expect(effortsFor(claude, sessionShown({ ...running, model: "claude-opus-5-5", model_request: "opus" }, branch, [claude]).values.model).map((e) => e.id)).toEqual(["low", "high"]);
  });

  // verifies: FLOW-NEW-SESSION-R12
  it("offer every effort for a model the host no longer lists, so the session's effort still shows", () => {
    const shown = sessionShown({ ...running, model: "retired", effort: "low" }, branch, [backend("claude")]);
    expect(shown.values.effort).toBe("low");
    expect(effortsFor(shown.backend, "retired").map((e) => e.id)).toEqual(["low", "high"]);
  });

  it("show a value left to the backend as the backend's default, and the root workspace as the project folder", () => {
    const shown = sessionShown({ ...running, model: undefined }, { ...branch, kind: "root" }, [backend("claude")]);
    expect(shown.values.model).toBe("claude-large");
    expect(shown.values.workspace).toBe("root");
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
    expect(next).toEqual({ shownFor: "c", seededWith: used("c"), touched: false, wanted: { ...used("c"), project_id: "c" }, picked: {} });
  });
});
