// @vitest-environment jsdom
// A question answered in a DOM, from the shelf and from the transcript's question card: what each shows, when its
// answers are available, and what each sends.

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { Envelope, QuestionAsked } from "../protocol/agent-events.gen";
import type { Reply } from "../session/answers";
import type { ShelfShows } from "../session/shelf";
import { EVENT_VERSION, apply, emptyTranscript } from "../session/transcript";
import { Shelf } from "./Shelf";
import { Transcript } from "./Transcript";

const options = [
  { id: "o1", label: "Windows only", description: "Where it flakes" },
  { id: "o2", label: "Everywhere" },
  { id: "o3", label: "Nowhere" },
];
const questions: QuestionAsked["questions"] = [
  { id: "q1", header: "Retry", text: "Where should the test retry?", options, multi_select: false, takes_text: false },
  { id: "q2", text: "Which runners should it run on?", options, multi_select: true, takes_text: false },
  { id: "q3", text: "What should the branch be called?", multi_select: false, takes_text: false },
];

const asked: Envelope = {
  v: EVENT_VERSION,
  session_id: "s1",
  seq: 0,
  ts: "2026-11-13T09:00:00Z",
  source: { backend: "claude-code", backend_version: "2.1.316", input: "live" },
  event: { type: "question.asked", request_id: "r1", questions },
};

type Sent = [string, Reply][];

function inShelf(sent: Sent, live = true) {
  render(
    createElement(Shelf, {
      tile: { title: "Fix the build" },
      shows: { state: "question", requestId: "r1", questions },
      live,
      onDecide: () => {},
      onAnswer: (requestId: string, reply: Reply) => sent.push([requestId, reply]),
    }),
  );
  return screen.getByRole("dialog");
}

function inTranscript(sent: Sent) {
  render(
    createElement(Transcript, {
      title: "Fix the build",
      view: { id: "s1", transcript: apply(emptyTranscript, asked), stream: { state: "live" } },
      onDecide: () => {},
      onAnswer: (requestId: string, reply: Reply) => sent.push([requestId, reply]),
    }),
  );
  return screen.getByRole("region", { name: "Question" });
}

/** Answers every question: an option, two options, and own words. */
function answerAll(surface: HTMLElement) {
  const q1 = within(surface).getByRole("radiogroup", { name: "Where should the test retry?" });
  fireEvent.click(within(q1).getByRole("radio", { name: /Everywhere/ }));
  const q2 = within(surface).getByRole("group", { name: "Which runners should it run on?" });
  fireEvent.click(within(q2).getByRole("checkbox", { name: /Nowhere/ }));
  fireEvent.click(within(q2).getByRole("checkbox", { name: /Windows only/ }));
  fireEvent.change(within(surface).getByRole("textbox", { name: "What should the branch be called?" }), { target: { value: "retry-on-windows" } });
}

const sentAnswers: Reply = {
  answers: [
    { id: "q1", value: "o2", own_words: false },
    { id: "q2", value: ["o1", "o3"], own_words: false },
    { id: "q3", value: "retry-on-windows", own_words: true },
  ],
};

afterEach(cleanup);

describe.each([
  ["the shelf", inShelf],
  ["the transcript's question card", inTranscript],
])("a question in %s", (_, show) => {
  // verifies: FLOW-APPROVE-PERMISSION-R15
  // verifies: FLOW-APPROVE-PERMISSION-R17
  it("shows each question's header, text and options with their descriptions, as choices of one or of any number", () => {
    const surface = show([]);
    expect(within(surface).getByText("Retry")).toBeTruthy();
    const q1 = within(surface).getByRole("radiogroup", { name: "Where should the test retry?" });
    expect(within(q1).getAllByRole("radio").map((r) => r.closest("label")?.textContent)).toEqual(["Windows onlyWhere it flakes", "Everywhere", "Nowhere"]);
    const q2 = within(surface).getByRole("group", { name: "Which runners should it run on?" });
    expect(within(q2).getAllByRole("checkbox")).toHaveLength(3);
    // A text field only where the question has no options or takes text beside them.
    expect(within(surface).getAllByRole("textbox")).toEqual([within(surface).getByRole("textbox", { name: "What should the branch be called?" })]);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R16
  // verifies: FLOW-APPROVE-PERMISSION-R17
  it("offers Answer once every question has an answer, and sends one answer per question", () => {
    const sent: Sent = [];
    const surface = show(sent);
    const answer = within(surface).getByRole("button", { name: "Answer" }) as HTMLButtonElement;
    expect(answer.disabled).toBe(true);
    answerAll(surface);
    expect(answer.disabled).toBe(false);
    fireEvent.click(answer);
    expect(sent).toEqual([["r1", sentAnswers]]);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R28
  it("offers Decline at once, which sends no answers", () => {
    const sent: Sent = [];
    const surface = show(sent);
    const decline = within(surface).getByRole("button", { name: "Decline" }) as HTMLButtonElement;
    expect(decline.disabled).toBe(false);
    fireEvent.click(decline);
    expect(sent).toEqual([["r1", "decline"]]);
  });
});

describe("a question that takes text beside its options", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R15
  it("offers a text field beside the options when the question takes text", () => {
    const sent: Sent = [];
    render(
      createElement(Shelf, {
        tile: { title: "Fix the build" },
        shows: { state: "question", requestId: "r2", questions: [{ ...(questions[0] as QuestionAsked["questions"][number]), takes_text: true }] },
        live: true,
        onDecide: () => {},
        onAnswer: (requestId: string, reply: Reply) => sent.push([requestId, reply]),
      }),
    );
    const field = screen.getByRole("textbox", { name: "Where should the test retry?" });
    fireEvent.change(field, { target: { value: "Only on the nightly run" } });
    fireEvent.click(screen.getByRole("button", { name: "Answer" }));
    expect(sent).toEqual([["r2", { answers: [{ id: "q1", value: "Only on the nightly run", own_words: true }] }]]);
  });
});

describe("a question's answers while Ersa can't reach Core", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R20
  it("are unavailable, Decline included, and the shelf says it is reconnecting", () => {
    const surface = inShelf([], false);
    answerAll(surface);
    expect((within(surface).getByRole("button", { name: "Decline" }) as HTMLButtonElement).disabled).toBe(true);
    expect((within(surface).getByRole("button", { name: "Answer" }) as HTMLButtonElement).disabled).toBe(true);
    expect(surface.textContent).toContain("Reconnecting to Core…");
  });
});

describe("a question's choices in the shelf", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R16
  it("last while the shelf loads again and as it settles after the answer", () => {
    const shelf = (shows: ShelfShows) =>
      createElement(Shelf, { tile: { title: "Fix the build" }, shows, live: true, onDecide: () => {}, onAnswer: () => {} });
    const question: ShelfShows = { state: "question", requestId: "r1", questions };
    const { rerender } = render(shelf(question));
    answerAll(screen.getByRole("dialog"));
    // A reconnect reads the session's events again.
    rerender(shelf({ state: "loading" }));
    rerender(shelf(question));
    const chosen = () => (screen.getByRole("radio", { name: /Everywhere/ }) as HTMLInputElement).checked;
    expect(chosen()).toBe(true);
    expect((screen.getByRole("button", { name: "Answer" }) as HTMLButtonElement).disabled).toBe(false);
    // Answered: the shelf shows the last request, its answers unavailable, until it closes.
    rerender(shelf({ state: "settled" }));
    expect(chosen()).toBe(true);
    expect((screen.getByRole("textbox", { name: "What should the branch be called?" }) as HTMLInputElement).value).toBe("retry-on-windows");
  });
});

describe("the transcript's question card once answered", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R28
  it("says the user declined, not that the question was answered", () => {
    const answered = (declined: boolean): Envelope => ({
      ...asked,
      seq: 1,
      event: { type: "question.answered", request_id: "r1", answers: [], declined, answered_by: "user" },
    });
    const card = (declined: boolean) => {
      cleanup();
      render(
        createElement(Transcript, {
          title: "Fix the build",
          view: { id: "s1", transcript: apply(apply(emptyTranscript, asked), answered(declined)), stream: { state: "live" } },
          onDecide: () => {},
          onAnswer: () => {},
        }),
      );
      return screen.getByRole("region", { name: "Question" }).textContent;
    };
    expect(card(true)).toContain("Question, declined");
    expect(card(false)).toContain("Question, answered");
  });
});
