// @vitest-environment jsdom
// The new-session screen in a DOM: where it starts while the projects' order arrives, and where the user's own
// choice, writing or sending holds it.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { BackendInfo, Choice, Project } from "../protocol/core-api.gen";

import { NewSessionScreen } from "./NewSessionScreen";

const host = { id: "local", name: "devbox", platform: "windows" as const };
const project = (id: string): Project => ({ id, host_id: "local", path: `D:/code/${id}`, name: id, vcs: "git", trusted: true });
const [alpha, beta, gamma] = [project("alpha"), project("beta"), project("gamma")];

const choices = (ids: string[]): Choice[] => ids.map((id, i) => ({ id, label: id, default: i === 0 }));
const claude: BackendInfo = {
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

describe("adding a project from the project chip", () => {
  /** Chooses Add project… from the open project chip, and lets the dialog answer. */
  const addFromChip = async () => {
    fireEvent.click(screen.getByRole("button", { name: /^Project:/ }));
    await act(async () => {
      fireEvent.click(screen.getByRole("option", { name: /Add project…/ }));
    });
  };

  // verifies: FLOW-NEW-SESSION-R42
  // verifies: FLOW-NEW-SESSION-R53
  it("says it is checking a picked folder, then adding it, with the ring, in the status line's place, sending nothing", () => {
    const { rerender } = render(createElement(NewSessionScreen, props([alpha], { adding: { state: "finding" }, initialText: "Fix the build" })));
    const line = () => document.querySelector(".new-session-above .composer-status-line");
    expect(line()?.textContent).toBe("Checking the folder…");
    expect(line()?.getAttribute("role")).toBe("status");
    expect(line()?.querySelector('.context-ring[data-turning=""]')).not.toBeNull();
    // The composer's own status line gives way: one line at a time.
    expect(document.querySelectorAll(".composer-status-line")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Send" }).getAttribute("aria-disabled")).toBe("true");
    rerender(createElement(NewSessionScreen, props([alpha], { adding: { state: "adding" }, initialText: "Fix the build" })));
    expect(line()?.textContent).toBe("Adding the project…");
    expect(line()?.querySelector('.context-ring[data-turning=""]')).not.toBeNull();
    rerender(createElement(NewSessionScreen, props([alpha], { initialText: "Fix the build" })));
    expect(line()).toBeNull();
  });

  // verifies: FLOW-NEW-SESSION-R53
  it("says Ersa can't reach the sessions in place of the folder's wait, which that line comes before", () => {
    render(createElement(NewSessionScreen, props([alpha], { adding: { state: "adding" }, away: "reconnecting" })));
    const lines = [...document.querySelectorAll(".composer-status-line")].map((l) => l.textContent);
    expect(lines).toEqual(["Ersa can't reach your sessions. It's trying again."]);
  });

  // verifies: FLOW-NEW-SESSION-R27
  it("offers the folder dialog only on the local host: a remote host's folders are not this computer's", async () => {
    const remote = { id: "far", name: "buildbox", platform: "linux" as const };
    const faraway: Project = { ...project("faraway"), host_id: "far", path: "/srv/faraway" };
    const onChooseFolder = vi.fn(async () => ({ kind: "canceled" }) as const);
    render(createElement(NewSessionScreen, props([faraway, alpha], { hosts: [host, remote], projectId: "faraway", onChooseFolder })));
    await addFromChip();
    expect(screen.getByRole("option", { name: /Add project…/ }).getAttribute("aria-disabled")).toBe("true");
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "o", ctrlKey: true });
    expect(onChooseFolder).not.toHaveBeenCalled();
  });

  // verifies: FLOW-NEW-SESSION-R27
  it("names a Windows folder by its own name in the trust question", async () => {
    const onChooseFolder = async () => ({ kind: "new", path: "D:\\Clones\\stranger" }) as const;
    render(createElement(NewSessionScreen, props([alpha], { onChooseFolder })));
    await addFromChip();
    const question = screen.getByRole("region", { name: "Trust stranger?" });
    expect(question.textContent).toContain("D:\\Clones\\stranger");
  });

  // verifies: FLOW-NEW-SESSION-R42
  it("sends nothing while the trust question is open, and sends again once it is answered", async () => {
    const onStart = vi.fn(async () => false);
    const onChooseFolder = async () => ({ kind: "new", path: "D:/clones/stranger" }) as const;
    render(createElement(NewSessionScreen, props([alpha], { onChooseFolder, onStart, initialText: "Fix the build" })));
    await addFromChip();
    const words = screen.getByRole("textbox", { name: "Message" });
    words.focus();
    await act(async () => {
      fireEvent.keyDown(words, { key: "Enter" });
      fireEvent.click(screen.getByRole("button", { name: "Send" }));
    });
    expect(onStart).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Send" }).getAttribute("aria-disabled")).toBe("true");
    expect(screen.getByRole("region", { name: "Trust stranger?" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await act(async () => {
      fireEvent.keyDown(words, { key: "Enter" });
    });
    expect(onStart).toHaveBeenCalledTimes(1);
  });

  // verifies: FLOW-NEW-SESSION-R17
  it("cancels the trust question on Escape from the message box, leaving everything as it was", async () => {
    const onAddProject = vi.fn(async () => undefined);
    const onChooseFolder = async () => ({ kind: "new", path: "D:/clones/stranger" }) as const;
    render(createElement(NewSessionScreen, props([alpha], { onChooseFolder, onAddProject, initialText: "Fix the build" })));
    await addFromChip();
    const words = screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
    words.focus();
    await act(async () => {
      fireEvent.keyDown(words, { key: "Escape" });
    });
    expect(screen.queryByRole("region", { name: /^Trust / })).toBeNull();
    expect(onAddProject).not.toHaveBeenCalled();
    expect(chosen()).toBe("Project: alpha");
    expect(words.value).toBe("Fix the build");
    expect(document.activeElement).toBe(words);
  });

  // verifies: FLOW-NEW-SESSION-R17
  it("leaves the trust question open on an Escape meant for something else", async () => {
    const onChooseFolder = async () => ({ kind: "new", path: "D:/clones/stranger" }) as const;
    render(createElement(NewSessionScreen, props([alpha], { onChooseFolder })));
    await addFromChip();
    // From outside the screen, such as the quit question in the rail.
    const elsewhere = document.body.appendChild(document.createElement("button"));
    try {
      fireEvent.keyDown(elsewhere, { key: "Escape" });
      expect(screen.getByRole("region", { name: "Trust stranger?" })).toBeTruthy();
      // An open chip's list, which closes itself instead.
      fireEvent.click(screen.getByRole("button", { name: /^Model:/ }));
      fireEvent.keyDown(screen.getByRole("listbox"), { key: "Escape" });
      expect(screen.queryByRole("listbox")).toBeNull();
      expect(screen.getByRole("region", { name: "Trust stranger?" })).toBeTruthy();
    } finally {
      elsewhere.remove();
    }
  });

  // verifies: FLOW-NEW-SESSION-R18
  it("chooses a found project even before Core's list has it", async () => {
    const beyond: Project = project("beyond");
    const onChooseFolder = async () => ({ kind: "found", project: beyond }) as const;
    render(createElement(NewSessionScreen, props([alpha], { onChooseFolder })));
    await addFromChip();
    expect(chosen()).toBe("Project: beyond");
  });
});

describe("what the user chose on the new-session screen when the project changes", () => {
  const offering: BackendInfo = {
    ...claude,
    models: [
      { id: "default", label: "Default (recommended)", default: true, efforts: ["low", "high"] },
      { id: "opus", label: "Opus 5.5", default: false, efforts: ["low", "high"] },
    ],
    efforts: ["low", "high"].map((id) => ({ id, label: id, default: false })),
    permission_modes: [
      { id: "default", label: "Manual", default: true },
      { id: "plan", label: "Plan", default: false },
    ],
  };
  const pick = (chip: string, option: RegExp) => {
    fireEvent.click(screen.getByRole("button", { name: new RegExp(`^${chip}:`) }));
    fireEvent.click(screen.getByRole("option", { name: option }));
  };
  const label = (chip: string) => screen.getByRole("button", { name: new RegExp(`^${chip}:`) }).getAttribute("aria-label");

  // verifies: FLOW-NEW-SESSION-R02
  // verifies: FLOW-NEW-SESSION-R19
  // verifies: FLOW-NEW-SESSION-R32
  it("keeps the model, effort and permission mode chosen before the project was added, and starts with them", async () => {
    const beyond = project("beyond");
    const onStart = vi.fn(async () => true);
    // The project last started at the defaults, as the first session of a new install leaves it.
    const lastUsed = { beyond: { host_id: "local", project_id: "beyond", workspace: "isolated" as const, backend: "claude-code", model: "default", permission_mode: "default" } };
    render(createElement(NewSessionScreen, props([], { backends: { local: [offering] }, lastUsed, onStart, onChooseFolder: async () => ({ kind: "found", project: beyond }) as const })));
    pick("Model", /Opus 5.5/);
    pick("Effort", /low/);
    pick("Permission mode", /Plan/);
    fireEvent.click(screen.getByRole("button", { name: /^Project:/ }));
    await act(async () => {
      fireEvent.click(screen.getByRole("option", { name: /Add project…/ }));
    });
    expect(chosen()).toBe("Project: beyond");
    expect([label("Model"), label("Effort"), label("Permission mode")]).toEqual(["Model: Opus 5.5", "Effort: low", "Permission mode: Plan"]);

    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Fix the build" } });
    await act(async () => {
      fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    });
    expect(onStart).toHaveBeenCalledWith("Fix the build", expect.objectContaining({ project_id: "beyond", model: "opus", effort: "low", permission_mode: "plan" }), []);
  });

  // verifies: FLOW-NEW-SESSION-R02
  // verifies: FLOW-NEW-SESSION-R32
  it("keeps them when another project is chosen, and takes the rest from what that project last used", () => {
    const lastUsed = { beta: { host_id: "local", project_id: "beta", workspace: "root" as const, backend: "claude-code", model: "opus", effort: "high", permission_mode: "default" } };
    render(createElement(NewSessionScreen, props([alpha, beta], { backends: { local: [offering] }, lastUsed })));
    pick("Permission mode", /Plan/);
    pick("Project", /beta/);
    expect((screen.getByRole("checkbox", { name: "Worktree" }) as HTMLInputElement).checked).toBe(false);
    expect([label("Model"), label("Effort"), label("Permission mode")]).toEqual(["Model: Opus 5.5", "Effort: high", "Permission mode: Plan"]);
    // Default effort chosen on the screen is a choice too: it stays none over the project's last level.
    pick("Effort", /Default/);
    pick("Project", /alpha/);
    pick("Project", /beta/);
    expect([label("Effort"), label("Permission mode")]).toEqual(["Effort: Default", "Permission mode: Plan"]);
  });
});

describe("a backend chosen on the new-session screen", () => {
  // verifies: FLOW-NEW-SESSION-R02
  // verifies: FLOW-NEW-SESSION-R32
  it("drops the model and permission mode chosen for the backend before it, so they don't follow a project change", () => {
    const other: BackendInfo = {
      ...claude,
      name: "copilot",
      label: "Copilot",
      models: [
        { id: "auto", label: "Auto", default: true },
        { id: "opus", label: "Opus via Copilot", default: false },
      ],
      permission_modes: [
        { id: "manual", label: "Manual", default: true },
        { id: "plan", label: "Plan", default: false },
      ],
    };
    const mine: BackendInfo = {
      ...claude,
      models: [
        { id: "default", label: "Default (recommended)", default: true },
        { id: "opus", label: "Opus 5.5", default: false },
      ],
      permission_modes: [
        { id: "default", label: "Manual", default: true },
        { id: "plan", label: "Plan", default: false },
      ],
    };
    render(createElement(NewSessionScreen, props([alpha, beta], { backends: { local: [mine, other] } })));
    const label = (chip: string) => screen.getByRole("button", { name: new RegExp(`^${chip}:`) }).getAttribute("aria-label");
    const pick = (chip: string, option: RegExp) => {
      fireEvent.click(screen.getByRole("button", { name: new RegExp(`^${chip}:`) }));
      fireEvent.click(screen.getByRole("option", { name: option }));
    };
    pick("Model", /Opus 5.5/);
    pick("Permission mode", /Plan/);
    pick("Backend", /Copilot/);
    pick("Project", /beta/);
    expect([label("Backend"), label("Model"), label("Permission mode")]).toEqual(["Backend: Copilot", "Model: Auto", "Permission mode: Manual"]);
  });
});

describe("the effort chip on the new-session screen", () => {
  // verifies: FLOW-NEW-SESSION-R02
  it("goes back to no level when Default is chosen, and keeps a level only while the model takes it", () => {
    const backend: BackendInfo = {
      ...claude,
      models: [
        { id: "default", label: "Default (recommended)", default: true, efforts: ["low", "high"] },
        { id: "haiku-4-5", label: "Haiku 4.5", default: false },
      ],
      efforts: ["low", "high"].map((id) => ({ id, label: id, default: false })),
    };
    render(createElement(NewSessionScreen, props([alpha], { backends: { local: [backend] } })));
    const effort = () => screen.getByRole("button", { name: /^Effort:/ });
    expect(effort().getAttribute("aria-label")).toBe("Effort: Default");

    fireEvent.click(effort());
    fireEvent.click(screen.getByRole("option", { name: /high/ }));
    expect(effort().getAttribute("aria-label")).toBe("Effort: high");
    fireEvent.click(effort());
    fireEvent.click(screen.getByRole("option", { name: /Default/ }));
    expect(effort().getAttribute("aria-label")).toBe("Effort: Default");

    // A model that takes no effort: the chip stays, unavailable, reading None.
    fireEvent.click(screen.getByRole("button", { name: /^Model:/ }));
    fireEvent.click(screen.getByRole("option", { name: /Haiku 4.5/ }));
    expect(effort().getAttribute("aria-label")).toBe("Effort: None");
    expect(effort().hasAttribute("disabled")).toBe(true);
  });
});

// The trust question beneath the sign-in note (docs/design/COMPOSER.md, The sign-in note): with the note the room above
// the box is in the column and the question takes its place there; when the note goes, the question fits itself
// afresh, so the room is not held at the question's height in the column with no note to keep it there.
describe("the trust question when the sign-in note goes", () => {
  it("fits itself afresh, so the room above the box goes back to opening upward", () => {
    const signedOut = { local: [{ ...claude, status: "signed_out" as const }] };
    const { rerender } = render(createElement(NewSessionScreen, props([alpha], { backends: signedOut, initialAsking: "D:\Clones\stranger" })));
    const section = screen.getByRole("region", { name: "New session" });
    expect(section.hasAttribute("data-noted")).toBe(true);
    expect(section.hasAttribute("data-tight")).toBe(true);
    rerender(createElement(NewSessionScreen, props([alpha], { initialAsking: "D:\Clones\stranger" })));
    expect(section.hasAttribute("data-noted")).toBe(false);
    expect(section.hasAttribute("data-tight")).toBe(false);
  });
});
