// The new-session screen rendered to markup, as the window would first draw it. Opening, choosing and Escape
// need a browser; the gallery's keyboard test in e2e/ covers them.

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Project } from "../protocol/core-api.gen";
import type { BackendChoices, Choice } from "../session/controls";
import { NewSessionScreen } from "./NewSessionScreen";

const host = { id: "local", name: "WORKSTATION", platform: "windows" as const };
const ersa: Project = { id: "p1", host_id: "local", path: "D:\\Projects\\ersa", name: "ersa", vcs: "git", trusted: true };
const notes: Project = { id: "p2", host_id: "local", path: "C:\\notes", name: "notes", vcs: "none", trusted: false };

const choices = (pairs: [string, string][], dflt: string): Choice[] =>
  pairs.map(([id, label]) => ({ id, label, default: id === dflt }));

const claude = (status: BackendChoices["status"] = "available"): BackendChoices => ({
  name: "claude-code",
  version: "2.1.302",
  status,
  tested: { min: "2.1.0", max: "2.1.400" },
  models: choices([["sonnet", "Sonnet"], ["opus", "Opus"]], "opus"),
  efforts: choices([["medium", "Medium"], ["high", "High"]], "high"),
  permission_modes: choices([["default", "Ask before acting"], ["acceptEdits", "Accept edits"]], "default"),
});
const copilot = (status: BackendChoices["status"]): BackendChoices => ({
  ...claude(),
  name: "copilot",
  version: undefined,
  status,
});

const render = (props: Partial<Parameters<typeof NewSessionScreen>[0]>) =>
  renderToStaticMarkup(
    createElement(NewSessionScreen, {
      hosts: [host],
      projects: [ersa, notes],
      backends: { local: [claude()] },
      onStart: async () => false,
      onAddProject: async () => false,
      ...props,
    }),
  );

/** Each chip's accessible name, "Workspace: New workspace", in order. */
const chips = (html: string) => [...html.matchAll(/class="chip"[^>]*aria-label="([^"]*)"|aria-label="([^"]*)"[^>]*class="chip"/g)].map((m) => m[1] ?? m[2] ?? "");

describe("the new-session screen", () => {
  // verifies: FLOW-NEW-SESSION-R01
  it("shows the wordmark, the composer focused, and the controls the session will start with", () => {
    const html = render({});
    expect(html).toContain(">Ersa</h1>");
    expect(html).toMatch(/<textarea[^>]*autofocus/i);
    expect(chips(html)).toEqual([
      "Host: WORKSTATION",
      "Project: ersa",
      "Workspace: New workspace",
      "Model: Opus",
      "Effort: High",
      "Permission mode: Ask before acting",
    ]);
    expect(html).toContain('aria-label="Context: none used yet"');
  });

  // verifies: FLOW-NEW-SESSION-R01
  it("shows the project New session was chosen for, at the values last used there", () => {
    const html = render({
      projectId: "p1",
      lastUsed: {
        p1: { host_id: "local", project_id: "p1", workspace: "root", backend: "claude-code", model: "sonnet", effort: "medium", permission_mode: "acceptEdits" },
      },
    });
    expect(chips(html)).toContain("Workspace: Root workspace");
    expect(chips(html)).toContain("Model: Sonnet");
    expect(chips(html)).toContain("Permission mode: Accept edits");
  });

  // verifies: FLOW-NEW-SESSION-R06
  it("offers the root workspace only for a project without a workspace adapter", () => {
    const html = render({ projectId: "p2", open: "workspace" });
    expect(chips(html)).toContain("Workspace: Root workspace");
    expect(html).not.toContain("New workspace");
  });

  // verifies: FLOW-NEW-SESSION-R07
  it("lists a missing or failed backend with its status rather than hiding it", () => {
    const html = render({ backends: { local: [claude(), copilot("missing")] }, open: "backend" });
    expect(chips(html)).toContain("Backend: Claude Code");
    expect(html).toMatch(/aria-disabled="true"[^>]*>.*GitHub Copilot.*Not installed/);
    const failed = render({ backends: { local: [claude(), copilot("error")] }, open: "backend" });
    expect(failed).toContain("Detection failed");
  });

  // verifies: FLOW-NEW-SESSION-R07
  it("shows a host's lone backend that is missing, and does not let a message start", () => {
    const html = render({ backends: { local: [claude("missing")] }, open: "backend" });
    expect(chips(html)).toContain("Backend: No backend");
    expect(html).toContain("Not installed");
    expect(html).toContain("No backend is available on this host.");
    expect(html).toMatch(/<textarea[^>]*disabled/);
  });

  it("hides the backend chip when the host has one backend", () => {
    expect(chips(render({})).some((c) => c.startsWith("Backend"))).toBe(false);
  });

  // verifies: FLOW-NEW-SESSION-R10
  it("offers to add a project when there is none, and nothing else", () => {
    const html = render({ projects: [] });
    expect(html).toContain('aria-label="Add a project"');
    expect(html).not.toContain("<textarea");
    expect(chips(html)).toEqual([]);
  });
});
