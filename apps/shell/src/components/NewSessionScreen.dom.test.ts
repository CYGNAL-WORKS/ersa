// @vitest-environment jsdom
// The new-session screen in a DOM: where it starts while the projects' order arrives, and where the user's own
// choice, writing or sending holds it.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { Project } from "../protocol/core-api.gen";
import type { BackendChoices, Choice } from "../session/controls";
import { NewSessionScreen } from "./NewSessionScreen";

const host = { id: "local", name: "devbox", platform: "windows" as const };
const project = (id: string): Project => ({ id, host_id: "local", path: `D:/code/${id}`, name: id, vcs: "git", trusted: true });
const [alpha, beta, gamma] = [project("alpha"), project("beta"), project("gamma")];

const choices = (ids: string[]): Choice[] => ids.map((id, i) => ({ id, label: id, default: i === 0 }));
const claude: BackendChoices = {
  name: "claude-code",
  label: "Claude Code",
  version: "2.1.302",
  status: "available",
  tested: { min: "2.1.0", max: "2.1.400" },
  models: choices(["opus"]),
  efforts: choices(["high"]),
  permission_modes: choices(["default"]),
};

type Props = Parameters<typeof NewSessionScreen>[0];
const props = (projects: Project[], more: Partial<Props> = {}): Props => ({
  hosts: [host],
  localHostId: "local",
  projects,
  backends: { local: [claude] },
  onStart: async () => false,
  onAddProject: async () => undefined,
  ...more,
});

/** The project the screen will start the session in, as its chip says. */
const chosen = () => screen.getByRole("button", { name: /^Project:/ }).getAttribute("aria-label");

afterEach(cleanup);

describe("where the new-session screen stays while the projects' order arrives", () => {
  // verifies: FLOW-NEW-SESSION-R15
  it("moves to the most recently used project when the order arrives after it opened", () => {
    const { rerender } = render(createElement(NewSessionScreen, props([alpha, beta])));
    expect(chosen()).toBe("Project: alpha");
    rerender(createElement(NewSessionScreen, props([beta, alpha])));
    expect(chosen()).toBe("Project: beta");
  });

  // verifies: FLOW-NEW-SESSION-R15
  it("keeps a project the user chose", () => {
    const { rerender } = render(createElement(NewSessionScreen, props([alpha, beta, gamma])));
    fireEvent.click(screen.getByRole("button", { name: /^Project:/ }));
    fireEvent.click(screen.getByRole("option", { name: /gamma/ }));
    expect(chosen()).toBe("Project: gamma");
    rerender(createElement(NewSessionScreen, props([beta, alpha, gamma])));
    expect(chosen()).toBe("Project: gamma");
  });

  // verifies: FLOW-NEW-SESSION-R15
  it("stays where it was once the user writes", () => {
    const { rerender } = render(createElement(NewSessionScreen, props([alpha, beta])));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Fix the build" } });
    rerender(createElement(NewSessionScreen, props([beta, alpha])));
    expect(chosen()).toBe("Project: alpha");
  });

  // verifies: FLOW-NEW-SESSION-R15
  it("stays where it was once a message is sent, so a failed send keeps it there", async () => {
    // The message is already written (a failed send gives it back), so sending is the only thing the user does.
    const { rerender } = render(createElement(NewSessionScreen, props([alpha, beta], { initialText: "Fix the build" })));
    await act(async () => {
      fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    });
    rerender(createElement(NewSessionScreen, props([beta, alpha], { initialText: "Fix the build" })));
    expect(chosen()).toBe("Project: alpha");
  });
});
