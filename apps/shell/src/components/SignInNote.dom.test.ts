// @vitest-environment jsdom
// The sign-in note's rows (docs/design/COMPOSER.md, The sign-in note): each state's glyph, words and one button, the
// button keeping keyboard focus through its changes, and nothing in the note that takes typing.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NoteState } from "../session/signIn";
import { SignInNote } from "./SignInNote";

afterEach(cleanup);

const draw = (state: NoteState, more: { onSignIn?: () => void; onCancel?: () => void } = {}) =>
  render(createElement(SignInNote, { label: "Claude Code", state, onSignIn: more.onSignIn ?? (() => {}), onCancel: more.onCancel ?? (() => {}) }));
const note = () => document.querySelector(".sign-in-note") as HTMLElement;
const words = () => note().querySelector(".sign-in-words")?.textContent;
const tone = () => note().querySelector(".glyph")?.getAttribute("data-tone");
/** The one button, by the label it shows. */
const button = () => note().querySelector("button") as HTMLButtonElement;
const shownLabel = () => button().querySelector(".button-label[data-shown]")?.textContent;

describe("the sign-in note", () => {
  // verifies: FLOW-SIGN-IN-R01
  it("says by the backend's label that it isn't signed in, with Sign in as the primary button and a caution glyph", () => {
    const signIn = vi.fn();
    draw({ kind: "signedOut" }, { onSignIn: signIn });
    expect(words()).toBe("Claude Code isn't signed in on this host. Sign in to start a session.");
    expect(tone()).toBe("caution");
    expect(screen.getByRole("button", { name: "Sign in" }).getAttribute("data-variant")).toBe("primary");
    fireEvent.click(button());
    expect(signIn).toHaveBeenCalledTimes(1);
  });

  // verifies: FLOW-SIGN-IN-R03
  it("reads Starting… while Core answers: the same words, unavailable yet focusable, doing nothing when chosen", () => {
    const signIn = vi.fn();
    draw({ kind: "starting" }, { onSignIn: signIn });
    expect(words()).toBe("Claude Code isn't signed in on this host. Sign in to start a session.");
    const starting = screen.getByRole("button", { name: "Starting…" });
    expect(starting.getAttribute("aria-disabled")).toBe("true");
    expect(starting.hasAttribute("disabled")).toBe(false);
    fireEvent.click(starting);
    expect(signIn).not.toHaveBeenCalled();
  });

  // verifies: FLOW-SIGN-IN-R06
  it("says it waits for the person in their browser, by the backend's label, as a status, with Cancel as a plain button", () => {
    const cancel = vi.fn();
    draw({ kind: "waiting" }, { onCancel: cancel });
    expect(words()).toBe("Waiting for you to sign in to Claude Code in your browser…");
    expect(note().querySelector(".context-ring[data-turning]")).not.toBeNull();
    expect(screen.getByRole("status").textContent).toBe("Waiting for you to sign in to Claude Code in your browser…");
    const cancelButton = screen.getByRole("button", { name: "Cancel" });
    expect(cancelButton.getAttribute("data-variant")).toBe("quiet");
    fireEvent.click(cancelButton);
    expect(cancel).toHaveBeenCalledTimes(1);
  });

  it.each<[string, NoteState, string]>([
    ["a refused start, with Core's reason", { kind: "signedOut", said: { kind: "refused", reason: "Ersa is shutting down." } }, "Claude Code's sign-in couldn't be started. Ersa is shutting down."],
    ["a page Ersa couldn't open", { kind: "signedOut", said: { kind: "unopened" } }, "Ersa couldn't open Claude Code's sign-in page."],
    ["a sign-in that failed, with the backend's reason", { kind: "signedOut", said: { kind: "failed", reason: "The browser was closed." } }, "Claude Code's sign-in didn't finish. The browser was closed."],
    ["a sign-in that failed with no reason", { kind: "signedOut", said: { kind: "failed" } }, "Claude Code's sign-in didn't finish."],
    ["a sign-in that took too long", { kind: "signedOut", said: { kind: "timedOut" } }, "Claude Code's sign-in took too long and was stopped."],
    ["a sign-in that ended still signed out", { kind: "signedOut", said: { kind: "stillSignedOut" } }, "The sign-in finished, but Claude Code still reports that it isn't signed in."],
  ])("says %s as an alert with a danger glyph, offering Sign in", (_, state, said) => {
    draw(state);
    expect(words()).toBe(said);
    expect(tone()).toBe("danger");
    expect(screen.getByRole("alert").textContent).toBe(said);
    expect(screen.getByRole("button", { name: "Sign in" }).getAttribute("data-variant")).toBe("primary");
  });

  // verifies: FLOW-SIGN-IN-R03
  it("says its own sentence alone for a refused start whose reason is empty, with nothing after it", () => {
    draw({ kind: "signedOut", said: { kind: "refused", reason: "" } });
    expect(words()).toBe("Claude Code's sign-in couldn't be started.");
    expect(screen.getByRole("alert").textContent).toBe("Claude Code's sign-in couldn't be started.");
  });

  it("holds all three labels in one cell, so the button keeps its width, with the ring leading Starting… alone", () => {
    draw({ kind: "signedOut" });
    const labels = [...button().querySelectorAll(".button-label")].map((l) => ({ text: l.textContent, ring: l.querySelector(".button-ring") !== null }));
    expect(labels).toEqual([
      { text: "Sign in", ring: false },
      { text: "Starting…", ring: true },
      { text: "Cancel", ring: false },
    ]);
  });

  it("keeps keyboard focus on its one button from Sign in through Starting… and Cancel to a failure", () => {
    const { rerender } = draw({ kind: "signedOut" });
    const first = button();
    first.focus();
    const again = (state: NoteState) => rerender(createElement(SignInNote, { label: "Claude Code", state, onSignIn: () => {}, onCancel: () => {} }));
    for (const state of [{ kind: "starting" }, { kind: "waiting" }, { kind: "signedOut", said: { kind: "timedOut" } }] as NoteState[]) {
      again(state);
      expect(button()).toBe(first);
      expect(document.activeElement).toBe(first);
    }
    expect(shownLabel()).toBe("Sign in");
  });

  // verifies: FLOW-SIGN-IN-R08
  it("offers no field in any state: nothing takes a password, a key, a token or a code", () => {
    const states: NoteState[] = [
      { kind: "signedOut" },
      { kind: "starting" },
      { kind: "waiting" },
      { kind: "signedOut", said: { kind: "refused", reason: "No." } },
      { kind: "signedOut", said: { kind: "unopened" } },
      { kind: "signedOut", said: { kind: "failed", reason: "No." } },
      { kind: "signedOut", said: { kind: "timedOut" } },
      { kind: "signedOut", said: { kind: "stillSignedOut" } },
    ];
    for (const state of states) {
      const { unmount } = draw(state);
      expect(note().querySelectorAll("input, textarea, select, [contenteditable]")).toHaveLength(0);
      expect(note().querySelectorAll("button")).toHaveLength(1);
      unmount();
    }
  });
});
