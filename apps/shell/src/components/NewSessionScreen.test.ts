// The new-session screen rendered to markup, as the window would first draw it. What it does as the user acts
// and data arrives is in NewSessionScreen.dom.test.ts; Escape and the keyboard are in the gallery's test in e2e/.

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Project } from "../protocol/core-api.gen";
import type { BackendChoices, Choice } from "../session/controls";
import { ContextRing } from "./ContextRing";
import { NewSessionScreen } from "./NewSessionScreen";

const host = { id: "local", name: "WORKSTATION", platform: "windows" as const };
const ersa: Project = { id: "p1", host_id: "local", path: "C:\\Users\\me\\ersa", name: "ersa", vcs: "git", trusted: true };
const notes: Project = { id: "p2", host_id: "local", path: "C:\\notes", name: "notes", vcs: "none", trusted: false };

const choices = (pairs: [string, string][], dflt: string): Choice[] =>
  pairs.map(([id, label]) => ({ id, label, default: id === dflt }));

const claude = (status: BackendChoices["status"] = "available"): BackendChoices => ({
  name: "claude-code",
  label: "Claude Code",
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
  label: "GitHub Copilot",
  version: undefined,
  status,
});

const render = (props: Partial<Parameters<typeof NewSessionScreen>[0]>) =>
  renderToStaticMarkup(
    createElement(NewSessionScreen, {
      hosts: [host],
      localHostId: "local",
      projects: [ersa, notes],
      backends: { local: [claude()] },
      onStart: async () => false,
      onAddProject: async () => undefined,
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
      "Host: Local",
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
    expect(chips(html)).toContain("Workspace: Project folder");
    expect(chips(html)).toContain("Model: Sonnet");
    expect(chips(html)).toContain("Permission mode: Accept edits");
  });

  // verifies: FLOW-NEW-SESSION-R14
  it("lists the host's projects in the order given, each with its name, path, kind and trust, the chosen one marked, then Add project", () => {
    const html = render({ projectId: "p2", open: "project" });
    const list = html.slice(html.indexOf('role="listbox"'));
    const options = [...list.matchAll(/<li[^>]*role="option"[^>]*>(.*?)<\/li>/g)].map((m) => m[0]);
    expect(options).toHaveLength(3);
    const [first, second, add] = options as [string, string, string];
    expect(first).toContain(">ersa<");
    expect(first).toContain(`>${ersa.path}<`);
    expect(first).toContain(">Git<");
    expect(first).not.toContain("Not trusted");
    expect(first).toContain('aria-selected="false"');
    expect(second).toContain(">notes<");
    expect(second).toContain(`>${notes.path}<`);
    expect(second).toContain(">Folder<");
    expect(second).toContain("Not trusted");
    expect(second).toContain('aria-selected="true"');
    // Without a folder dialog (none is given here) Add project is shown, unavailable, and without its shortcut.
    expect(add).toContain("Add project…");
    expect(add).not.toContain("Ctrl+O");
    expect(add).toContain('aria-disabled="true"');
  });

  // The host filter predates the project list's two lines; it cites nothing so R14's tests are the ones this change adds.
  it("lists only the chosen host's projects", () => {
    const far: Project = { ...ersa, id: "p9", host_id: "far", name: "faraway" };
    const html = render({ projects: [ersa, far, notes], open: "project" });
    expect(html).not.toContain("faraway");
  });

  // verifies: FLOW-NEW-SESSION-R15
  it("chooses the local host's first listed project, the most recently used, when opened for no project", () => {
    const far: Project = { ...ersa, id: "p9", host_id: "far", name: "faraway" };
    expect(chips(render({ projects: [far, notes, ersa] }))).toContain("Project: notes");
  });

  // verifies: FLOW-NEW-SESSION-R06
  it("offers the root workspace only for a project without a workspace adapter", () => {
    const html = render({ projectId: "p2", open: "workspace" });
    expect(chips(html)).toContain("Workspace: Project folder");
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

  // verifies: FLOW-NEW-SESSION-R23
  it("says above the box while the host's backends are detected, and the box is unavailable until they are", () => {
    const detecting = render({ backends: {} });
    expect(detecting).toMatch(/class="composer-status-line">Detecting the backends on this host…</);
    expect(detecting).toMatch(/<textarea[^>]*disabled/);
    const detected = render({});
    expect(detected).not.toContain("Detecting the backends on this host…");
    expect(detected).not.toMatch(/<textarea[^>]*disabled/);
  });

  it("opens the trust question in the status line's place, the box's focus shown by its edge alone", () => {
    const html = render({ backends: {}, initialAsking: "D:/clones/stranger" });
    expect(html).toContain("Trust stranger?");
    expect(html).not.toContain("composer-status-line");
    expect(html).toMatch(/class="composer-box"[^>]*data-quiet-focus/);
  });

  // verifies: FLOW-NEW-SESSION-R07
  it("shows a host's lone backend that is missing, and does not let a message start", () => {
    const html = render({ backends: { local: [claude("missing")] }, open: "backend" });
    expect(chips(html)).toContain("Backend: No backend");
    expect(html).toContain("Not installed");
    expect(html).toContain("No backend is available on this host. Install a backend on it to start a session.");
    expect(html).toMatch(/<textarea[^>]*disabled/);
  });

  // verifies: FLOW-NEW-SESSION-R07
  it("names a backend that failed detection, by its label, when none can run", () => {
    const copilot: BackendChoices = { ...claude("missing"), name: "copilot", label: "GitHub Copilot" };
    const html = render({ backends: { local: [copilot, { ...claude("error"), label: "Claude Code" }] } });
    // The markup escapes the apostrophes.
    expect(html.replaceAll("&#x27;", "'")).toContain("Claude Code isn't working on this host. Check that it's installed and signed in, then open New session again.");
    expect(html).not.toContain("No backend is available on this host.");
    expect(html).toMatch(/<textarea[^>]*disabled/);
  });

  it("hides the backend chip when the host has one backend", () => {
    expect(chips(render({})).some((c) => c.startsWith("Backend"))).toBe(false);
  });

  it("leaves out the chip for a choice the backend does not offer", () => {
    const html = render({ backends: { local: [{ ...claude(), efforts: [] }] } });
    expect(chips(html).some((c) => c.startsWith("Effort"))).toBe(false);
    expect(chips(html)).toContain("Model: Opus");
  });

  it("lists a host without projects as one that has nothing to start", () => {
    const remote = { id: "box", name: "BUILDBOX", platform: "linux" as const };
    const html = render({ hosts: [host, remote], open: "host" });
    expect(html).toMatch(/aria-disabled="true"[^>]*>.*BUILDBOX.*No projects/);
  });
});

describe("the context ring", () => {
  it("reads a value it cannot measure as none used", () => {
    const html = renderToStaticMarkup(createElement(ContextRing, { used: Number.NaN }));
    expect(html).toContain("Context: none used yet");
    expect(html).not.toContain("NaN");
  });
});
