// @vitest-environment jsdom
// The composer in a DOM (docs/design/COMPOSER.md; FLOW-SEND-MESSAGE, FLOW-NEW-SESSION-R23): sending, what stays in it,
// and the status line above its box.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { Sending } from "../session/follow";
import { Composer, type Unavailable } from "./Composer";

afterEach(cleanup);

type Props = {
  unavailable?: Unavailable;
  sending?: Sending;
  quietFocus?: boolean;
  working?: boolean;
  onStop?: () => void;
  onSend?: (text: string) => Promise<boolean>;
};
const draw = (props: Props = {}) => createElement(Composer, { autoFocus: false, onSend: async () => true, ...props });
const words = () => screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
const statusLine = () => document.querySelector(".composer-status-line");
const write = (text: string) => fireEvent.change(words(), { target: { value: text } });

describe("sending from the composer", () => {
  // verifies: FLOW-SEND-MESSAGE-R01
  it("sends the words with Enter and clears them once accepted; Shift+Enter does not send", async () => {
    const sent: string[] = [];
    render(draw({ onSend: async (text) => (sent.push(text), true) }));
    write("Fix the build");
    fireEvent.keyDown(words(), { key: "Enter", shiftKey: true });
    expect(sent).toEqual([]);
    await act(async () => {
      fireEvent.keyDown(words(), { key: "Enter" });
    });
    expect(sent).toEqual(["Fix the build"]);
    expect(words().value).toBe("");
  });

  // verifies: FLOW-SEND-MESSAGE-R01
  it("sends with the send button, which offers nothing while there is nothing to send", async () => {
    const sent: string[] = [];
    render(draw({ onSend: async (text) => (sent.push(text), true) }));
    const send = screen.getByRole("button", { name: "Send" }) as HTMLButtonElement;
    // Still focusable with nothing to send, so focus on stop survives the turn ending.
    expect(send.getAttribute("aria-disabled")).toBe("true");
    fireEvent.click(send);
    expect(sent).toEqual([]);
    write("Run the tests");
    expect(send.getAttribute("aria-disabled")).toBeNull();
    await act(async () => {
      fireEvent.click(send);
    });
    expect(sent).toEqual(["Run the tests"]);
  });

  // verifies: FLOW-SEND-MESSAGE-R02
  it("keeps what is written while a message is on its way after that message is accepted", async () => {
    let accept: (ok: boolean) => void = () => {};
    render(draw({ onSend: () => new Promise((r) => (accept = r)) }));
    write("First");
    fireEvent.keyDown(words(), { key: "Enter" });
    write("Second");
    await act(async () => accept(true));
    expect(words().value).toBe("Second");
  });
});

describe("the status line above the box", () => {
  // verifies: FLOW-SEND-MESSAGE-R05
  it("shows a refused message's reason as an alert, led by the danger glyph, and keeps the message", () => {
    const { rerender } = render(draw());
    write("Fix the build");
    rerender(draw({ sending: { state: "failed", message: "The session has ended." } }));
    const line = screen.getByRole("alert");
    expect(line.textContent).toBe("The session has ended.");
    expect(line.querySelector('.glyph[data-tone="danger"]')).not.toBeNull();
    expect(line.closest(".composer-status")?.nextElementSibling?.classList.contains("composer-box")).toBe(true);
    expect(words().value).toBe("Fix the build");
    expect(words().disabled).toBe(false);
  });

  // verifies: FLOW-SEND-MESSAGE-R06
  it.each([
    ["reconnecting", "Ersa can't reach your sessions. It's trying again."],
    ["unreachable", "Ersa can't reach your sessions."],
  ] as const)("says Ersa can't reach the sessions (%s), with the box unavailable and the words kept", (state, says) => {
    const { rerender } = render(draw());
    write("Fix the build");
    rerender(draw({ unavailable: state }));
    expect(statusLine()?.textContent).toBe(says);
    expect(words().disabled).toBe(true);
    expect((screen.getByRole("button", { name: "Send" }) as HTMLButtonElement).disabled).toBe(true);
    expect(document.querySelector(".composer-box")?.hasAttribute("data-unavailable")).toBe(true);
    expect(words().value).toBe("Fix the build");
  });

  // verifies: FLOW-SEND-MESSAGE-R06
  it("says Ersa can't reach the sessions before a refused message, and the refusal after it reaches them", () => {
    const refused: Sending = { state: "failed", message: "The session has ended." };
    const { rerender } = render(draw({ unavailable: "reconnecting", sending: refused }));
    expect(statusLine()?.textContent).toBe("Ersa can't reach your sessions. It's trying again.");
    rerender(draw({ sending: refused }));
    expect(statusLine()?.textContent).toBe("The session has ended.");
  });

  it("returns focus to the words when the box is available again, if they had it and focus went nowhere since", () => {
    const { rerender } = render(draw());
    words().focus();
    rerender(draw({ unavailable: "reconnecting" }));
    // A browser drops focus from the disabled words to the page; jsdom keeps it there, so the test does it.
    const passing = document.body.appendChild(document.createElement("button"));
    passing.focus();
    passing.remove();
    expect(document.activeElement).toBe(document.body);
    rerender(draw());
    expect(document.activeElement).toBe(words());
  });

  it("returns focus to stop when the box is available again, if stop had it", () => {
    const { rerender } = render(draw({ working: true, onStop: () => {} }));
    const button = document.querySelector(".composer-send") as HTMLButtonElement;
    button.focus();
    rerender(draw({ working: true, onStop: () => {}, unavailable: "reconnecting" }));
    const passing = document.body.appendChild(document.createElement("button"));
    passing.focus();
    passing.remove();
    rerender(draw({ working: true, onStop: () => {} }));
    expect(document.activeElement).toBe(button);
  });

  it("leaves focus where the user moved it while the box was unavailable", () => {
    const elsewhere = document.body.appendChild(document.createElement("button"));
    const { rerender } = render(draw());
    words().focus();
    rerender(draw({ unavailable: "reconnecting" }));
    elsewhere.focus();
    rerender(draw());
    expect(document.activeElement).toBe(elsewhere);
    elsewhere.remove();
  });

  // verifies: FLOW-NEW-SESSION-R23
  it("says the backends are being detected, with the box unavailable", () => {
    render(draw({ unavailable: "detecting" }));
    expect(statusLine()?.textContent).toBe("Detecting the backends on this host…");
    expect(words().disabled).toBe(true);
  });

  it("says nothing, and takes no room, while there is nothing to say", () => {
    render(draw({ sending: { state: "sending" } }));
    expect(statusLine()).toBeNull();
  });

  it("shows focus by the box's edge alone while a permission request is open", () => {
    const { rerender } = render(draw());
    expect(document.querySelector(".composer-box")?.hasAttribute("data-quiet-focus")).toBe(false);
    rerender(draw({ quietFocus: true }));
    expect(document.querySelector(".composer-box")?.hasAttribute("data-quiet-focus")).toBe(true);
  });
});

describe("stop, while a message is on its way or the agent works", () => {
  const button = () => document.querySelector(".composer-send") as HTMLButtonElement;

  // verifies: FLOW-SEND-MESSAGE-R03
  it("offers stop from the moment of sending, and stop keeps the message", async () => {
    let stops = 0;
    const { rerender } = render(draw({ onStop: () => stops++ }));
    write("Fix the build");
    rerender(draw({ onStop: () => stops++, sending: { state: "sending" } }));
    expect(button().getAttribute("aria-label")).toBe("Stop");
    fireEvent.click(button());
    expect(stops).toBe(1);
    // The send is canceled and answers false, so the words stay.
    rerender(draw({ onStop: () => stops++ }));
    expect(words().value).toBe("Fix the build");
    expect(button().getAttribute("aria-label")).toBe("Send");
  });

  // verifies: FLOW-SEND-MESSAGE-R03
  it("offers stop while the agent works, until the turn ends, keeping keyboard focus on the button", () => {
    let stops = 0;
    const { rerender } = render(draw({ onStop: () => stops++, working: true }));
    expect(button().getAttribute("aria-label")).toBe("Stop");
    button().focus();
    fireEvent.click(button());
    expect(stops).toBe(1);
    rerender(draw({ onStop: () => stops++, working: false }));
    expect(button().getAttribute("aria-label")).toBe("Send");
    expect(document.activeElement).toBe(button());
  });

  // verifies: FLOW-SEND-MESSAGE-R04
  it("does not send with Enter while the agent works, and keeps what is written for when the turn ends", async () => {
    const sent: string[] = [];
    const { rerender } = render(draw({ working: true, onStop: () => {}, onSend: async (text) => (sent.push(text), true) }));
    write("Next, run the tests");
    await act(async () => {
      fireEvent.keyDown(words(), { key: "Enter" });
    });
    expect(sent).toEqual([]);
    expect(words().value).toBe("Next, run the tests");
    rerender(draw({ working: false, onStop: () => {}, onSend: async (text) => (sent.push(text), true) }));
    await act(async () => {
      fireEvent.keyDown(words(), { key: "Enter" });
    });
    expect(sent).toEqual(["Next, run the tests"]);
  });
});
