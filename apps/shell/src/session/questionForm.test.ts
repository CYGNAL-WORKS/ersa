import { describe, expect, it } from "vitest";
import type { QuestionAsked } from "../protocol/agent-events.gen";
import { type Draft, answersOf, choose, chosenOf, takesWords, wordsOf, write } from "./questionForm";

type Question = QuestionAsked["questions"][number];

const options = [
  { id: "o1", label: "Windows only" },
  { id: "o2", label: "Everywhere", description: "Linux and macOS too" },
  { id: "o3", label: "Nowhere" },
];
const single: Question = { id: "q1", text: "Where should it retry?", options, multi_select: false, takes_text: false };
const multi: Question = { id: "q2", text: "Which runners?", options, multi_select: true, takes_text: false };
const wordsOnly: Question = { id: "q3", text: "What should the branch be called?", multi_select: false, takes_text: false };
const both: Question = { id: "q4", text: "Which shell?", options, multi_select: false, takes_text: true };

describe("a question's form", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R15
  it("takes one option for a single-choice question and any number for one that allows several", () => {
    let draft: Draft = {};
    draft = choose(draft, single, "o1");
    draft = choose(draft, single, "o2");
    expect(chosenOf(draft, "q1")).toEqual(["o2"]);
    draft = choose(draft, multi, "o3");
    draft = choose(draft, multi, "o1");
    draft = choose(draft, multi, "o2");
    // Choosing a chosen one again takes it away.
    draft = choose(draft, multi, "o3");
    expect(chosenOf(draft, "q2")).toEqual(["o1", "o2"]);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R15
  it("shows a text field when a question has no options or takes text beside them", () => {
    expect(takesWords(wordsOnly)).toBe(true);
    expect(takesWords({ ...wordsOnly, options: [] })).toBe(true);
    expect(takesWords(both)).toBe(true);
    expect(takesWords(single)).toBe(false);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R16
  it("has answers only once every question has one", () => {
    let draft: Draft = {};
    expect(answersOf([single, multi, wordsOnly], draft)).toBeUndefined();
    draft = choose(draft, single, "o1");
    draft = choose(draft, multi, "o2");
    expect(answersOf([single, multi, wordsOnly], draft)).toBeUndefined();
    // Words that are only spacing are no answer.
    draft = write(draft, "q3", "  ");
    expect(answersOf([single, multi, wordsOnly], draft)).toBeUndefined();
    draft = write(draft, "q3", "retry-on-windows");
    expect(answersOf([single, multi, wordsOnly], draft)).toBeDefined();
    // Taking away the last option chosen leaves its question unanswered.
    draft = choose(draft, multi, "o2");
    expect(answersOf([single, multi, wordsOnly], draft)).toBeUndefined();
  });

  // verifies: FLOW-APPROVE-PERMISSION-R16
  it("answers each question with the chosen option's id, the chosen options' ids, or the user's own words", () => {
    let draft: Draft = {};
    draft = choose(draft, single, "o2");
    // Chosen out of order, sent in the question's order.
    draft = choose(draft, multi, "o3");
    draft = choose(draft, multi, "o1");
    draft = write(draft, "q3", " retry-on-windows ");
    expect(answersOf([single, multi, wordsOnly], draft)).toEqual([
      { id: "q1", value: "o2", own_words: false },
      { id: "q2", value: ["o1", "o3"], own_words: false },
      { id: "q3", value: " retry-on-windows ", own_words: true },
    ]);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R16
  it("answers with whichever the user gave last, its options or its own words", () => {
    let draft: Draft = choose({}, both, "o1");
    draft = write(draft, "q4", "PowerShell 7");
    expect(chosenOf(draft, "q4")).toEqual([]);
    expect(answersOf([both], draft)).toEqual([{ id: "q4", value: "PowerShell 7", own_words: true }]);
    draft = choose(draft, both, "o3");
    expect(wordsOf(draft, "q4")).toBe("");
    expect(answersOf([both], draft)).toEqual([{ id: "q4", value: "o3", own_words: false }]);
  });
});
