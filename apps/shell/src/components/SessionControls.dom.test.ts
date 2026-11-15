// @vitest-environment jsdom
// The Worktree checkbox in the session controls, in place of the workspace chip (issue 412).

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Project } from "../protocol/core-api.gen";
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
  // verifies: FLOW-NEW-SESSION-R29
  it("reads Worktree, checked for a new isolated workspace, says what it means, and stands where the workspace chip was", () => {
    const { box } = controls("isolated");
    expect(box()?.checked).toBe(true);
    expect(box()?.getAttribute("aria-description")).toBe("The session works in an isolated copy of the repository.");
    expect(box()?.closest("label")?.getAttribute("title")).toBe("The session works in an isolated copy of the repository.");
    expect(screen.queryByRole("button", { name: /^Workspace:/ })).toBeNull();
    // Where the session runs leads the row: host, project, then the checkbox.
    expect(box()?.closest(".session-controls-where")).not.toBeNull();
  });

  // verifies: FLOW-NEW-SESSION-R29
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
  it("shows, unavailable, whether a session runs in a worktree", () => {
    expect(controls("isolated", { readOnly: true }).box()).toMatchObject({ checked: true, disabled: true });
    cleanup();
    expect(controls("root", { readOnly: true }).box()).toMatchObject({ checked: false, disabled: true });
  });

  it("is unavailable while the first message is on its way", () => {
    // The browser sends a disabled box no click, so nothing can change it.
    expect(controls("isolated", { disabled: true }).box()?.disabled).toBe(true);
  });
});
