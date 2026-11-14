// The form a question is answered with (FLOW-APPROVE-PERMISSION-R15, R16): what the user has chosen or written for
// each of the request's questions so far, and the answers Core is sent once every question has one. Kept free of
// React, so every decision is made, and tested, here; the shelf and the transcript's question card lay it out.

import type { QuestionAnswered, QuestionAsked } from "../protocol/agent-events.gen";

type Question = QuestionAsked["questions"][number];

/** One question's answer so far: the options chosen, by id, or the user's own words. Never both, since an answer is one or the other. */
export type Draft = Readonly<Record<string, { chosen: readonly string[] } | { words: string }>>;

/** Whether a question offers options to choose from; with none it takes only the user's own words. */
export const hasOptions = (q: Question): boolean => (q.options?.length ?? 0) > 0;

/** Whether a question shows a text field for the user's own words: with no options, or when it takes text beside them (R15). */
export const takesWords = (q: Question): boolean => !hasOptions(q) || q.takes_text;

/** The option ids chosen for a question, none when it is answered in words or not yet. */
export function chosenOf(draft: Draft, id: string): readonly string[] {
  const entry = draft[id];
  return entry && "chosen" in entry ? entry.chosen : [];
}

/** The user's own words for a question, empty when it is answered with options or not yet. */
export function wordsOf(draft: Draft, id: string): string {
  const entry = draft[id];
  return entry && "words" in entry ? entry.words : "";
}

/**
 * The draft after the user chooses an option: for a single-choice question it replaces the choice, for a question
 * that allows several it is added or, when already chosen, taken away. Choosing an option sets aside the question's
 * own words, so what the form shows is the answer that will be sent.
 */
export function choose(draft: Draft, q: Question, optionId: string): Draft {
  const chosen = chosenOf(draft, q.id);
  let next: readonly string[];
  if (!q.multi_select) next = [optionId];
  else if (chosen.includes(optionId)) next = chosen.filter((c) => c !== optionId);
  else next = [...chosen, optionId];
  return { ...draft, [q.id]: { chosen: next } };
}

/**
 * The draft after the user writes in a question's text field. Writing sets aside the options chosen, so what the
 * form shows is the answer that will be sent; clearing the field leaves the question unanswered.
 */
export function write(draft: Draft, id: string, words: string): Draft {
  return { ...draft, [id]: { words } };
}

/** One question's answer, as `question.answered` holds it, or none while it has no answer. */
function answerOf(q: Question, draft: Draft): QuestionAnswered["answers"][number] | undefined {
  const words = wordsOf(draft, q.id);
  // Words that are only spacing say nothing, so they are no answer; words that say something are sent as typed.
  if (words.trim() !== "") return { id: q.id, value: words, own_words: true };
  const chosen = chosenOf(draft, q.id);
  if (chosen.length === 0) return undefined;
  // Several in the order the question offers them, whatever order they were chosen in.
  if (q.multi_select) return { id: q.id, value: (q.options ?? []).map((o) => o.id).filter((o) => chosen.includes(o)), own_words: false };
  return { id: q.id, value: chosen[0] as string, own_words: false };
}

/**
 * The answers Core is sent (R16): one per question, being the chosen option's id, the chosen options' ids, or the
 * user's own words with `own_words` set; none until every question in the request has an answer, which is when
 * Answer becomes available.
 */
export function answersOf(questions: readonly Question[], draft: Draft): QuestionAnswered["answers"] | undefined {
  const answers = questions.map((q) => answerOf(q, draft));
  return answers.every((a) => a !== undefined) ? (answers as QuestionAnswered["answers"]) : undefined;
}
