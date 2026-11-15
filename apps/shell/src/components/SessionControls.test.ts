// The session controls rendered to markup, as the window would first draw them (docs/design/COMPOSER.md).

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { BackendInfo, ModelChoice, Project } from "../protocol/core-api.gen";
import { SessionControls, modelOptions } from "./SessionControls";

const project: Project = { id: "p1", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true };
const host = { id: "local", name: "devbox", platform: "windows" } as const;

describe("the workspace list's base branch", () => {
  it("says the project has no branches, not that none matches, when the repository lists none", () => {
    const shown = renderToStaticMarkup(
      createElement(SessionControls, {
        hosts: [host],
        localHostId: "local",
        projects: [project],
        backends: [],
        values: { host_id: "local", project_id: "p1", workspace: "isolated" },
        branches: { state: "read", branches: [] },
        onChoose: () => {},
        open: "workspace",
      }),
    );
    expect(shown).toContain("This project has no branches yet.");
    expect(shown).not.toContain("No branch matches.");
  });
});

// The default model's name, as Claude Desktop's model chip gives it: Default, then the model the default resolves to.
describe("the model chip's default", () => {
  const fallback: ModelChoice = { id: "default", label: "Default (recommended)", default: true, resolved: "claude-opus-5-5" };
  const sonnet: ModelChoice = { id: "sonnet", label: "Sonnet 5.5", default: false, resolved: "claude-sonnet-5-5" };
  const listed: ModelChoice[] = [fallback, sonnet, { id: "opus", label: "Opus 5.5", default: false, resolved: "claude-opus-5-5" }];
  const labels = (models: ModelChoice[]) => modelOptions(models).map((o) => o.label);

  // verifies: FLOW-NEW-SESSION-R36
  it("names the listed model the default resolves to, and leaves every other label as the backend gives it", () => {
    expect(labels(listed)).toEqual(["Default (Opus 5.5)", "Sonnet 5.5", "Opus 5.5"]);
  });

  // verifies: FLOW-NEW-SESSION-R36
  it("keeps the backend's label for a default whose resolved id no other listed model has", () => {
    expect(labels([{ ...fallback, resolved: "claude-fable-5-2" }, sonnet])).toEqual(["Default (recommended)", "Sonnet 5.5"]);
  });

  // verifies: FLOW-NEW-SESSION-R36
  it("keeps the backend's label for a default with no resolved id", () => {
    expect(labels([{ id: "auto", label: "Auto", default: true }, sonnet])).toEqual(["Auto", "Sonnet 5.5"]);
  });
});

describe("the how chips as Claude Desktop shows them", () => {
  const backend: BackendInfo = {
    name: "claude-code",
    label: "Claude Code",
    version: "2.1.302",
    status: "available",
    tested: { min: "2.1.0", max: "2.1.400" },
    models: [
      { id: "default", label: "Default (recommended)", default: true, resolved: "claude-opus-5-5", efforts: ["medium", "high"] },
      { id: "opus", label: "Opus 5.5", default: false, resolved: "claude-opus-5-5", efforts: ["medium", "high"] },
    ],
    efforts: [
      { id: "medium", label: "Medium", default: false },
      { id: "high", label: "High", default: false },
    ],
    permission_modes: [{ id: "default", label: "Ask", default: true }],
  };
  const render = (effort?: string) =>
    renderToStaticMarkup(
      createElement(SessionControls, {
        hosts: [host],
        localHostId: "local",
        projects: [project],
        backends: [backend],
        values: { host_id: "local", project_id: "p1", workspace: "isolated", backend: "claude-code", model: "default", effort, permission_mode: "default" },
        onChoose: () => {},
      }),
    );
  /** What a chip shows: the text between its tags, joined, found by its accessible name's first words. */
  const shown = (html: string, name: string) => {
    const inner = html.match(new RegExp(`aria-label="${name}: [^"]*"[^>]*>(.*?)</button>`))?.[1] ?? "";
    return [...inner.matchAll(/>([^<]+)</g)].map((m) => m[1]).join("");
  };

  it("shows the effort's level alone, and Default with none chosen, while its name still says it is the effort", () => {
    const medium = render("medium");
    expect(shown(medium, "Effort")).toBe("Medium");
    expect(medium).toContain('aria-label="Effort: Medium"');
    const none = render();
    expect(shown(none, "Effort")).toBe("Default");
    expect(none).toContain('aria-label="Effort: Default"');
  });

  // verifies: FLOW-NEW-SESSION-R36
  it("shows the default model by the model it resolves to, and the permission mode by its name", () => {
    const html = render();
    expect(shown(html, "Model")).toBe("Default (Opus 5.5)");
    expect(shown(html, "Permission mode")).toBe("Ask");
  });
});
