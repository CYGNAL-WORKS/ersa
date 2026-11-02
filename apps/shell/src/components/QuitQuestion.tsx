import { useEffect, useRef } from "react";
import { Button } from "./Button";
import "./QuitQuestion.css";

interface QuitQuestionProps {
  /** How many sessions are working or waiting on the user, and would stop. */
  busy: number;
  /** The user chose Quit. */
  onQuit: () => void;
  /** Closes the question and leaves everything running: Cancel, or Escape. */
  onCancel: () => void;
  /** Quit was chosen and Ersa is stopping; the answer cannot change any more. */
  quitting?: boolean;
  /** Whether the question takes focus as it opens; false for the gallery, which shows several at once. */
  takeFocus?: boolean;
  /** For the gallery: Cancel shown focused, as the keyboard leaves it when the question opens. */
  forceCancelFocus?: boolean;
}

/** Asks before Quit stops sessions that are busy (FLOW-CLOSE-AND-QUIT-R04), naming how many. */
export function QuitQuestion({ busy, onQuit, onCancel, quitting = false, takeFocus = true, forceCancelFocus }: QuitQuestionProps) {
  // Focus starts on Cancel, so an Enter pressed out of habit keeps the sessions running.
  const cancel = useRef<HTMLButtonElement>(null);
  // Only as it opens, so the first value of `takeFocus` is the one that counts.
  const focusOnOpen = useRef(takeFocus);
  useEffect(() => {
    if (focusOnOpen.current) cancel.current?.focus({ preventScroll: true });
  }, []);
  return (
    <section
      className="quit-question"
      role="alertdialog"
      aria-label="Quit Ersa?"
      onKeyDown={(event) => {
        if (event.key === "Escape" && !quitting) onCancel();
      }}
    >
      <p className="quit-question-title">Quit Ersa?</p>
      <p className="quit-question-detail">
        <span className="readout">{busy}</span> {busy === 1 ? "session is" : "sessions are"} working or waiting for you, and will stop.
        Closing the window instead keeps them running.
      </p>
      <div className="quit-question-actions">
        <Button onClick={onQuit} disabled={quitting}>
          {quitting ? "Quitting…" : "Quit"}
        </Button>
        <Button ref={cancel} onClick={onCancel} disabled={quitting} force={forceCancelFocus ? "focus" : undefined}>
          Cancel
        </Button>
      </div>
    </section>
  );
}
