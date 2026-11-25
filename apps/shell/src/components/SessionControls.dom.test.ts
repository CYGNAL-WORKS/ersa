// @vitest-environment jsdom
// The Worktree checkbox in the session controls, in place of the workspace chip (issue 412).

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { BackendInfo, Project } from "../protocol/core-api.gen";
import { SessionControls } from "./SessionControls";

const project: Project = { id: "p1", host_id: "local", path: "D:/code/alpha", name: "alpha", vcs: "git", trusted: true };
const host = { id: "local", name: "devbox", platform: "windows" } as const;

afterEach(cleanup);

function controls(workspace: "isolated" | "root", more: { readOnly?: boolean; disabled?: boolean; projects?: Project[] } = {}) {
  const onChoose = vi.fn();
  render(
    createElement(SessionControls, {
      hosts: [host],
      localHostId: "local",
      projects: more.projects ?? [project],
      backends: [],
      values: { host_id: "local", project_id: "p1", workspace },
      onChoose,
      readOnly: more.readOnly,
      disabled: more.disabled,
    }),
  );
  return { onChoose, box: () => screen.queryByRole("checkbox", { name: "Worktree" }) as HTMLInputElement | null };
}

describe("the Worktree checkbox", () => {
  // verifies: FLOW-NEW-SESSION-R37
  it("reads Worktree, checked for a new isolated workspace, says what it means, and stands where the workspace chip was", () => {
    const { box } = controls("isolated");
    expect(box()?.checked).toBe(true);
    expect(box()?.getAttribute("aria-description")).toBe("The session works in an isolated copy of the repository.");
    expect(box()?.closest("label")?.getAttribute("title")).toBe("The session works in an isolated copy of the repository.");
    expect(screen.queryByRole("button", { name: /^Workspace:/ })).toBeNull();
    // Where the session runs leads the row: host, project, then the checkbox, straight after the project chip.
    const where = box()?.closest(".session-controls-where");
    const order = [...(where?.querySelectorAll("button.chip, label.worktree") ?? [])].map((el) => el.getAttribute("aria-label") ?? el.textContent);
    expect(order).toEqual(["Host: Local", "Project: alpha", "Worktree"]);
  });

  // verifies: FLOW-NEW-SESSION-R37
  it("unchecked, chooses the project folder; checked again, a new isolated workspace", () => {
    const { box, onChoose } = controls("isolated");
    fireEvent.click(box()!);
    expect(onChoose).toHaveBeenLastCalledWith("workspace", "root");
    cleanup();
    const root = controls("root");
    expect(root.box()?.checked).toBe(false);
    fireEvent.click(root.box()!);
    expect(root.onChoose).toHaveBeenLastCalledWith("workspace", "isolated");
  });

  // verifies: FLOW-NEW-SESSION-R06
  it("is absent for a project Ersa has no workspace adapter for", () => {
    expect(controls("root", { projects: [{ ...project, vcs: "none" }] }).box()).toBeNull();
  });

  // verifies: FLOW-NEW-SESSION-R12
  // verifies: FLOW-NEW-SESSION-R39
  it("shows, unavailable, whether a session runs in a worktree", () => {
    expect(controls("isolated", { readOnly: true }).box()).toMatchObject({ checked: true, disabled: true });
    cleanup();
    expect(controls("root", { readOnly: true }).box()).toMatchObject({ checked: false, disabled: true });
  });

  // verifies: FLOW-NEW-SESSION-R39
  it("is unavailable while the first message is on its way", () => {
    // The browser sends a disabled box no click, so nothing can change it.
    expect(controls("isolated", { disabled: true }).box()?.disabled).toBe(true);
  });
});

describe("the permission mode chip's list", () => {
  const backend: BackendInfo = {
    name: "claude-code",
    label: "Claude Code",
    status: "available",
    tested: { min: "2.1.0", max: "2.1.400" },
    models: [],
    efforts: [],
    permission_modes: [
      { id: "default", label: "Manual", default: true, description: "Always ask before making changes" },
      { id: "plan", label: "Plan", default: false, description: "Create a plan before making changes" },
      // A mode its adapter says nothing more of.
      { id: "custom", label: "Custom", default: false },
    ],
  };

  function openModes() {
    render(
      createElement(SessionControls, {
        hosts: [host],
        localHostId: "local",
        projects: [project],
        backends: [backend],
        values: { host_id: "local", project_id: "p1", workspace: "isolated", backend: "claude-code", permission_mode: "plan" },
        onChoose: vi.fn(),
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: /^Permission mode:/ }));
    return screen.getByRole("listbox", { name: "Permission mode" });
  }

  // What assistive technology hears after the option's name: the text of what it is described by.
  const described = (option: HTMLElement) =>
    (option.getAttribute("aria-describedby") ?? "")
      .split(" ")
      .filter(Boolean)
      .map((id) => document.getElementById(id)?.textContent ?? "")
      .join(" ");

  // verifies: FLOW-NEW-SESSION-R45
  it("shows each mode's description beneath its label, and the label alone for a mode with none", () => {
    const list = openModes();
    const manual = within(list).getByRole("option", { name: "Manual" });
    const plan = within(list).getByRole("option", { name: "Plan" });
    const custom = within(list).getByRole("option", { name: "Custom" });
    // The option is still named by its label; the sentence is what it says next.
    expect(described(manual)).toBe("Always ask before making changes");
    expect(described(plan)).toBe("Create a plan before making changes");
    // Beneath the label: the label first, then the description, each its own line of the option.
    const lines = (option: HTMLElement) =>
      [...option.querySelectorAll(".chip-option-label, .chip-option-description")].map((el) => el.textContent);
    expect(lines(manual)).toEqual(["Manual", "Always ask before making changes"]);
    expect(manual.dataset.described).toBeDefined();
    // No empty second line for a mode with no description.
    expect(lines(custom)).toEqual(["Custom"]);
    expect(custom.dataset.described).toBeUndefined();
    expect(custom.hasAttribute("aria-describedby")).toBe(false);
  });
});
