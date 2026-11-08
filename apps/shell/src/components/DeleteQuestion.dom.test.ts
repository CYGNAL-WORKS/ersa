// @vitest-environment jsdom
// The delete question as the user reads it (FLOW-ARCHIVE-DELETE-SESSION, step 4; docs/design/RAIL.md, The delete
// question): its words from a `workspace.changes` answer, its buttons in each phase, and where focus goes.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { Workspace, WorkspaceChanges } from "../protocol/core-api.gen";
import { type DeletePhase, whatGoes } from "../session/deletion";
import { DeleteQuestion } from "./DeleteQuestion";

afterEach(cleanup);

const isolated: Workspace = { id: "w1", project_id: "p1", kind: "isolated", path: "D:\\ws\\ersa-1", vcs_ref: "ersa/fix-build", state: "ready" };
const root: Workspace = { ...isolated, id: "w0", kind: "root", path: "D:\\Projects\\ersa", vcs_ref: "" };
const clean: WorkspaceChanges = { uncommitted: 0, ignored: 0, unshared: 0, stranded: 0, sessions: 1, unrecorded: false, token: "t" };

const calls: string[] = [];
function show(phase: Exclude<DeletePhase, { kind: "deleted" }>) {
  calls.length = 0;
  render(
    createElement(DeleteQuestion, {
      title: "Fix the build",
      backend: "Claude Code",
      phase,
      onConfirm: () => calls.push("confirm"),
      onTryAgain: () => calls.push("tryAgain"),
      onCancel: () => calls.push("cancel"),
    }),
  );
  return screen.getByRole("dialog");
}
const asking = (workspace: Workspace, changes?: Partial<WorkspaceChanges>, changed = false) =>
  show({ kind: "asking", goes: whatGoes(workspace, changes && { ...clean, ...changes }), changed });
const button = (name: string) => screen.getByRole("button", { name });

describe("the delete question's words", () => {
  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R04
  it("names the session, says the conversation goes for good, names the branch, and counts what is lost", () => {
    const dialog = asking(isolated, { uncommitted: 3, ignored: 1, unshared: 2, stranded: 2 });
    expect(dialog.textContent).toContain("Delete Fix the build?");
    expect(dialog.textContent).toContain("Its conversation is deleted for good, in Claude Code too.");
    expect(dialog.textContent).toContain("Its folder and its branch ersa/fix-build are deleted with it.");
    expect(dialog.textContent).toContain("3 uncommitted or untracked files, 1 ignored file, and 2 commits no branch holds will be lost.");
    expect(dialog.textContent).not.toContain("is kept");
    // The confirming button says it deletes them too.
    expect(button("Delete and lose them")).toBeTruthy();
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R04
  it("says the branch is kept and why when it holds commits that exist nowhere else", () => {
    const dialog = asking(isolated, { unshared: 4, stranded: 0 });
    expect(dialog.textContent).toContain("Its folder is deleted with it. Its branch ersa/fix-build is kept.");
    expect(dialog.textContent).toContain("The branch holds commits that exist nowhere else.");
    expect(dialog.textContent).not.toContain("will be lost");
    expect(button("Delete")).toBeTruthy();
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R04
  it("says the files stay, and where, for the project's own folder", () => {
    const dialog = asking(root);
    expect(dialog.textContent).toContain("Its files stay in D:\\Projects\\ersa.");
    expect(dialog.textContent).not.toContain("folder is deleted");
    expect(button("Delete")).toBeTruthy();
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R04
  it("says the files stay for a workspace another session uses, however much they hold", () => {
    const dialog = asking(isolated, { uncommitted: 7, sessions: 2 });
    expect(dialog.textContent).toContain("Its files stay in D:\\ws\\ersa-1.");
    expect(dialog.textContent).not.toContain("will be lost");
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R10
  it("says commits on a detached checkout can't be counted, in place of a count, when git lost the record", () => {
    const dialog = asking(isolated, { uncommitted: 2, unrecorded: true });
    expect(dialog.textContent).toContain(
      "2 uncommitted or untracked files will be lost. Commits made on a detached checkout can't be counted and may be lost with the folder.",
    );
    expect(dialog.textContent).not.toContain("no branch holds");
    expect(button("Delete and lose them")).toBeTruthy();
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R08
  it("says what would be lost changed when it asks again", () => {
    const dialog = asking(isolated, { uncommitted: 1 }, true);
    expect(dialog.textContent).toContain("What would be lost changed since you were asked.");
    expect(document.activeElement).toBe(button("Cancel"));
  });
});

describe("the delete question's buttons", () => {
  it("starts with focus on Cancel; Cancel and Escape cancel; confirming confirms", () => {
    const dialog = asking(isolated);
    expect(document.activeElement).toBe(button("Cancel"));
    fireEvent.click(button("Delete"));
    fireEvent.keyDown(dialog, { key: "Escape" });
    fireEvent.click(button("Cancel"));
    expect(calls).toEqual(["confirm", "cancel", "cancel"]);
  });

  it("keeps focus within it: Tab from the last button wraps to the first", () => {
    asking(isolated);
    button("Delete").focus();
    fireEvent.keyDown(button("Delete"), { key: "Tab" });
    expect(document.activeElement).toBe(button("Cancel"));
    fireEvent.keyDown(button("Cancel"), { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(button("Delete"));
  });

  it("makes both buttons and Escape unavailable while the deletion runs", () => {
    const dialog = show({ kind: "deleting", goes: whatGoes(isolated, clean) });
    expect((button("Cancel") as HTMLButtonElement).disabled).toBe(true);
    expect((button("Deleting…") as HTMLButtonElement).disabled).toBe(true);
    expect(document.activeElement).toBe(dialog);
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(calls).toEqual([]);
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R07
  it("shows a failure's reason with Try again, which takes focus", () => {
    const dialog = show({ kind: "failed", reason: "The folder is in use by another program." });
    expect(dialog.textContent).toContain("Ersa couldn't delete this session.");
    expect(dialog.textContent).toContain("The folder is in use by another program.");
    expect(document.activeElement).toBe(button("Try again"));
    fireEvent.click(button("Try again"));
    expect(calls).toEqual(["tryAgain"]);
  });

  it("keeps focus in the dialog through Try again, and on Cancel each time it asks", () => {
    const props = { title: "Fix the build", backend: "Claude Code", onConfirm: () => {}, onTryAgain: () => {}, onCancel: () => {} };
    const at = (phase: Exclude<DeletePhase, { kind: "deleted" }>) => createElement(DeleteQuestion, { ...props, phase });
    const { rerender } = render(at({ kind: "failed", reason: "The folder is in use." }));
    expect(document.activeElement).toBe(button("Try again"));
    rerender(at({ kind: "reading" }));
    expect(document.activeElement).toBe(button("Cancel"));
    rerender(at({ kind: "asking", changed: false, goes: whatGoes(isolated, clean) }));
    expect(document.activeElement).toBe(button("Cancel"));
    // Asked again twice in a row: focus goes back to Cancel each time.
    for (let i = 0; i < 2; i++) {
      (screen.getAllByRole("button")[1] as HTMLButtonElement).focus();
      rerender(at({ kind: "asking", changed: true, goes: whatGoes(isolated, { ...clean, uncommitted: i + 1 }) }));
      expect(document.activeElement).toBe(button("Cancel"));
    }
  });

  it("takes no focus away when the rest of the window is pressed, so Escape still reaches it", () => {
    const dialog = asking(isolated);
    // A press outside is held, so focus stays on Cancel; a press inside acts as usual.
    expect(fireEvent.mouseDown(document.body)).toBe(false);
    expect(document.activeElement).toBe(button("Cancel"));
    expect(fireEvent.mouseDown(button("Delete"))).toBe(true);
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(calls).toEqual(["cancel"]);
  });

  it("keeps its bottom edge where it opened, so the buttons hold still as it grows", () => {
    const dialog = show({ kind: "reading" });
    expect(dialog.hasAttribute("data-anchored")).toBe(true);
    expect(dialog.style.getPropertyValue("--delete-question-bottom")).toMatch(/px$/);
  });

  it("can't be confirmed while it reads what would be lost", () => {
    const dialog = show({ kind: "reading" });
    expect(dialog.textContent).toContain("Checking what would be lost…");
    expect((button("Delete") as HTMLButtonElement).disabled).toBe(true);
  });
});
