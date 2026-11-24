// The session controls rendered to markup, as the window would first draw them (docs/design/COMPOSER.md).

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { BackendInfo, Choice, ModelChoice, Project } from "../protocol/core-api.gen";
import { DEFAULT_EFFORT, SessionControls, effortChip, modelChip } from "./SessionControls";

const project: Project = { id: "p1", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true };
const host = { id: "local", name: "devbox", platform: "windows" } as const;

// The model chip and the backend's default (FLOW-NEW-SESSION-R46): where a listed model stands for the default, the chip
// reads and marks that model and the list has no row for the default; otherwise the default keeps the backend's label.
describe("the model chip's default", () => {
  const fallback: ModelChoice = { id: "default", label: "Default (recommended)", default: true, resolved: "claude-opus-5-5" };
  const sonnet: ModelChoice = { id: "sonnet", label: "Sonnet 5.5", default: false, resolved: "claude-sonnet-5-5" };
  const opus: ModelChoice = { id: "opus", label: "Opus 5.5", default: false, resolved: "claude-opus-5-5" };
  const opusFull: ModelChoice = { id: "claude-opus-5-5", label: "Opus 5.5 (full id)", default: false, resolved: "claude-opus-5-5" };
  const listed: ModelChoice[] = [fallback, sonnet, opus, opusFull];
  const labels = (models: ModelChoice[], model: string) => modelChip(models, model).options.map((o) => o.label);

  // verifies: FLOW-NEW-SESSION-R46
  it("leaves the default out of the list, and reads and marks the first listed model that stands for it", () => {
    expect(labels(listed, "default")).toEqual(["Sonnet 5.5", "Opus 5.5", "Opus 5.5 (full id)"]);
    expect(modelChip(listed, "default").value).toBe("opus");
    // A model the user chose reads as chosen, the one that stands for the default included, and the list is the same.
    expect(modelChip(listed, "sonnet").value).toBe("sonnet");
    expect(modelChip(listed, "claude-opus-5-5").value).toBe("claude-opus-5-5");
    expect(labels(listed, "sonnet")).toEqual(["Sonnet 5.5", "Opus 5.5", "Opus 5.5 (full id)"]);
  });

  // verifies: FLOW-NEW-SESSION-R46
  it("keeps the backend's label for a default whose resolved id no other listed model has", () => {
    const models = [{ ...fallback, resolved: "claude-fable-5-2" }, sonnet];
    expect(labels(models, "default")).toEqual(["Default (recommended)", "Sonnet 5.5"]);
    expect(modelChip(models, "default").value).toBe("default");
  });

  // verifies: FLOW-NEW-SESSION-R46
  it("keeps the backend's label for a default with no resolved id", () => {
    const models = [{ id: "auto", label: "Auto", default: true }, sonnet];
    expect(labels(models, "auto")).toEqual(["Auto", "Sonnet 5.5"]);
    expect(modelChip(models, "auto").value).toBe("auto");
  });
});

// The effort chip and the backend's default effort (FLOW-NEW-SESSION-R47): with none chosen it reads and marks the level
// the backend marks, in a list of the levels alone, and choosing that level names it; Default only where none is marked
// among the levels the model takes.
describe("the effort chip's default", () => {
  const level = (id: string, label: string, dflt = false): Choice => ({ id, label, default: dflt });
  const marked = [level("low", "Low"), level("medium", "Medium", true), level("high", "High")];
  const unmarked = marked.map((c) => ({ ...c, default: false }));
  const ids = (chip: ReturnType<typeof effortChip>) => chip.options.map((o) => o.id);

  // verifies: FLOW-NEW-SESSION-R47
  it("reads and marks the marked level with none chosen, in a list of the levels alone, before and under a session", () => {
    for (const running of [false, true]) {
      const chip = effortChip(marked, undefined, running);
      expect(chip.reads).toBeUndefined();
      // Shown as chosen, yet not chosen: choosing it passes it on.
      expect(chip.value).toBeUndefined();
      expect(chip.marked).toBe("medium");
      expect(ids(chip)).toEqual(["low", "medium", "high"]);
    }
  });

  // verifies: FLOW-NEW-SESSION-R47
  it("reads a chosen level as chosen beside a marked default, the marked one included, with no Default row", () => {
    for (const chosen of ["high", "medium"]) {
      const chip = effortChip(marked, chosen, false);
      expect(chip.value).toBe(chosen);
      expect(chip.reads).toBeUndefined();
    }
    expect(ids(effortChip(marked, "high", false))).toEqual(["low", "medium", "high"]);
  });

  // verifies: FLOW-NEW-SESSION-R47
  it("reads Default where the backend marks none, or the model takes levels and not the marked one", () => {
    // The levels a model that takes Low and High alone is offered: the marked Medium is not among them.
    const notTaken = marked.filter((c) => c.id !== "medium");
    for (const levels of [unmarked, notTaken]) {
      const before = effortChip(levels, undefined, false);
      expect(before).toMatchObject({ value: DEFAULT_EFFORT, reads: "Default" });
      expect(before.marked).toBeUndefined();
      expect(before.options[0]).toEqual({ id: DEFAULT_EFFORT, label: "Default" });
      expect(ids(before).slice(1)).toEqual(levels.map((c) => c.id));
      // Under a session the list holds only the levels.
      expect(ids(effortChip(levels, undefined, true))).toEqual(levels.map((c) => c.id));
    }
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
    permission_modes: [{ id: "default", label: "Manual", default: true }],
  };
  const render = (effort?: string, listed: BackendInfo = backend) =>
    renderToStaticMarkup(
      createElement(SessionControls, {
        hosts: [host],
        localHostId: "local",
        projects: [project],
        backends: [listed],
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

  // verifies: FLOW-NEW-SESSION-R47
  it("reads the level the backend marks as its default with none chosen", () => {
    const marked = { ...backend, efforts: backend.efforts.map((e) => ({ ...e, default: e.id === "medium" })) };
    const none = render(undefined, marked);
    expect(shown(none, "Effort")).toBe("Medium");
    expect(none).toContain('aria-label="Effort: Medium"');
  });

  // verifies: FLOW-NEW-SESSION-R46
  // verifies: FLOW-NEW-SESSION-R48
  it("names the lone backend, the default model by the listed model it stands for, and the permission mode", () => {
    const html = render();
    expect(shown(html, "Backend")).toBe("Claude Code");
    expect(shown(html, "Model")).toBe("Opus 5.5");
    expect(shown(html, "Permission mode")).toBe("Manual");
  });
});

