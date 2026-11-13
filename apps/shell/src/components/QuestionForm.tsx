import { Check, LoaderCircle } from "lucide-react";
import { type MouseEvent, type ReactNode, useId } from "react";
import { t } from "../catalog";
import type { QuestionAsked } from "../protocol/agent-events.gen";
import type { Answer, Reply } from "../session/answers";
import { type Draft, answersOf, choose, chosenOf, hasOptions, takesWords, wordsOf, write } from "../session/questionForm";
import { Button } from "./Button";
import { Icon } from "./Icon";
import { Revealed } from "./Transcript";
// The answers' row, notes and ring are the ones a permission request's answers use.
import "./PermissionAnswers.css";
import "./QuestionForm.css";

interface QuestionFormProps {
  questions: QuestionAsked["questions"];
  /** The answer on its way, or refused, for this request. */
  answer?: Answer;
  /** Whether answers can reach Core now. */
  live: boolean;
  /** Says Ersa is reconnecting while it can't reach Core, unless the surface around it already does. */
  reconnectingNote?: boolean;
  /** What sits between the questions and the answers: the shelf's readouts (R23). */
  between?: ReactNode;
  onAnswer: (reply: Reply) => void;
  /** What the user has chosen or written so far, held by the surface around the form so it outlasts the form: the
   *  shelf redraws its contents as it loads again or settles, and the choices must still be there. */
  draft: Draft;
  onDraft: (update: (draft: Draft) => Draft) => void;
}

/**
 * A question and its answers (FLOW-APPROVE-PERMISSION-R15, R16, R28): each of the request's questions with its header,
 * its text and its options, choices of one or of any number, and a text field for the user's own words where the
 * question takes them; then Decline, available at once, and Answer, available once every question has an answer. The
 * shelf and the transcript's question card both offer them so (R17). docs/design/RAIL.md, The shelf: A question.
 */
export function QuestionForm({ questions, answer, live, reconnectingNote = true, between, onAnswer, draft, onDraft: setDraft }: QuestionFormProps) {
  const name = useId();
  const sending = answer?.state === "sending" ? answer.decision : undefined;
  // One answer at a time, and none while Core can't be reached (R18, R20).
  const unavailable = sending !== undefined || !live;
  const answers = answersOf(questions, draft);
  // Choosing an answer makes every answer unavailable, so focus moves to the surface around them (the shelf, or the
  // transcript's card, each focusable outside the tab order) rather than fall to the page with the button.
  const send = (reply: Reply) => (event: MouseEvent<HTMLButtonElement>) => {
    const holder = event.currentTarget.closest<HTMLElement>('[tabindex="-1"]');
    onAnswer(reply);
    holder?.focus();
  };
  // The answer on its way leads its label with a ring.
  const ring = (declined: boolean) =>
    sending !== undefined &&
    (sending === "decline") === declined && (
      <span className="answers-ring" aria-hidden="true">
        <Icon icon={LoaderCircle} size="icon-xs" />
      </span>
    );
  return (
    <>
      {questions.map((q, i) => {
        const textId = `${name}-${i}`;
        const chosen = chosenOf(draft, q.id);
        return (
          // A single choice is heard as a radio group; anything else as a group of its controls.
          <div key={q.id} className="question" role={hasOptions(q) && !q.multi_select ? "radiogroup" : "group"} aria-labelledby={textId}>
            {q.header && (
              <p className="question-header">
                <Revealed text={q.header} />
              </p>
            )}
            <p id={textId} className="question-text">
              <Revealed text={q.text} />
            </p>
            {hasOptions(q) && (
              <div className="question-options">
                {q.options?.map((o) => {
                  const on = chosen.includes(o.id);
                  return (
                    <label key={o.id} className="question-option" data-chosen={on || undefined} data-unavailable={unavailable || undefined}>
                      {/* The native control keeps the keyboard and what assistive technology hears; the row is its look. */}
                      <input
                        className="question-control"
                        type={q.multi_select ? "checkbox" : "radio"}
                        name={`${name}-${q.id}`}
                        checked={on}
                        disabled={unavailable}
                        onChange={() => setDraft((d) => choose(d, q, o.id))}
                      />
                      <span className="question-option-label">
                        <Revealed text={o.label} />
                        {o.description && (
                          <span className="question-option-description">
                            <Revealed text={o.description} />
                          </span>
                        )}
                      </span>
                      {/* Its place is kept, so choosing an option moves nothing. */}
                      <span className="question-check" aria-hidden="true">
                        {on && <Icon icon={Check} size="icon-sm" />}
                      </span>
                    </label>
                  );
                })}
              </div>
            )}
            {takesWords(q) && (
              <input
                className="question-words"
                aria-labelledby={textId}
                placeholder={t("rail.shelf.ownWordsPlaceholder")}
                value={wordsOf(draft, q.id)}
                readOnly={unavailable}
                onChange={(event) => {
                  const words = event.target.value;
                  setDraft((d) => write(d, q.id, words));
                }}
              />
            )}
          </div>
        );
      })}
      {between}
      <div className="answers" aria-busy={sending !== undefined || undefined}>
        {!live && reconnectingNote && (
          <p className="answers-note" role="status">
            {t("rail.shelf.reconnectingNote")}
          </p>
        )}
        {live && answer?.state === "failed" && (
          <div className="answers-refused" role="alert">
            <p className="problem">{t("rail.shelf.answerRefusedError")}</p>
            <p className="answers-reason">{answer.message}</p>
          </div>
        )}
        <div className="answers-row">
          <Button variant="quiet" disabled={unavailable} onClick={send("decline")}>
            {ring(true)}
            {t("rail.shelf.declineButton")}
          </Button>
          <Button variant="primary" disabled={unavailable || answers === undefined} onClick={answers && send({ answers })}>
            {ring(false)}
            {t("rail.shelf.answerButton")}
          </Button>
        </div>
      </div>
    </>
  );
}
