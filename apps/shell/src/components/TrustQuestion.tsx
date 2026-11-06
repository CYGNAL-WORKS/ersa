import { useEffect, useRef } from "react";
import { t } from "../catalog";
import { Button } from "./Button";
import "./TrustQuestion.css";

interface TrustQuestionProps {
  /** The project's name, as the question names it. */
  name: string;
  /** The folder the user picked, shown when the project is not added yet, so they see exactly what they trust. */
  path?: string;
  /**
   * Adding a project: both answers add it, trusted or not, and the labels say so. Changing an added project's trust:
   * trusting is the one answer, since not trusting it is leaving it as it is.
   */
  purpose: "add" | "change";
  /** The user's answer: whether the project's own configuration may run in its sessions. */
  onAnswer: (trusted: boolean) => void;
  /** Closes the question without an answer: Cancel, or Escape. */
  onDismiss: () => void;
  /** Whether the question takes focus as it opens; true when the user opened it, false for a question shown from the start. */
  takeFocus?: boolean;
  /** For the gallery: Cancel shown focused, as the keyboard leaves it when the question opens. */
  forceCancelFocus?: boolean;
}

/**
 * Asks whether the user trusts a project, saying plainly what trust lets run, that it covers every workspace of the
 * project, and that it lasts until withdrawn (ADR 0018, FLOW-NEW-SESSION-R16, retired; #226 builds its replacement).
 */
export function TrustQuestion({ name, path, purpose, onAnswer, onDismiss, takeFocus = true, forceCancelFocus }: TrustQuestionProps) {
  // The control that opened the question is disabled or elsewhere while it shows, so focus moves here rather than
  // to the page: to Cancel, so a second Enter cancels and never trusts by accident.
  const cancel = useRef<HTMLButtonElement>(null);
  // Only as it opens, so the first value of `takeFocus` is the one that counts.
  const focusOnOpen = useRef(takeFocus);
  useEffect(() => {
    if (focusOnOpen.current) cancel.current?.focus({ preventScroll: true });
  }, []);
  const title = t("trust.question.title", { name });
  return (
    <section
      className="trust-question"
      // Adding opens above the composer's box as a card that holds the screen's focal glow (docs/design/COMPOSER.md,
      // The trust question); changing an added project's trust opens in the rail, under the project.
      data-place={purpose === "add" ? "composer" : "rail"}
      aria-label={title}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          // The question is the thing Escape closes, not whatever holds it.
          event.stopPropagation();
          onDismiss();
        }
      }}
    >
      <p className="trust-question-title">{title}</p>
      {path !== undefined && <p className="trust-question-path mono">{path}</p>}
      {/* The explanation scrolls when the question has less room than it needs; the title, the folder and the
          answers above and below it always show. */}
      <div className="trust-question-body">
        <p className="trust-question-detail">{t("trust.question.runsDetail")}</p>
        <p className="trust-question-detail">{t("trust.question.coversDetail")}</p>
        <p className="trust-question-detail">{t("trust.question.untrustedDetail")}</p>
      </div>
      <div className="trust-question-actions">
        {/* Neither answer is the primary one: Ersa does not lean towards trust. */}
        {purpose === "add" ? (
          <>
            <Button onClick={() => onAnswer(true)}>{t("trust.question.addTrustedButton")}</Button>
            <Button onClick={() => onAnswer(false)}>{t("trust.question.addUntrustedButton")}</Button>
          </>
        ) : (
          <Button onClick={() => onAnswer(true)}>{t("trust.question.trustButton")}</Button>
        )}
        <Button ref={cancel} onClick={onDismiss} force={forceCancelFocus ? "focus" : undefined}>
          {t("trust.question.cancelButton")}
        </Button>
      </div>
    </section>
  );
}

/** A folder's last name, for a question about a project not yet added. */
export function folderName(path: string): string {
  // Windows paths separate with a backslash, others with a slash.
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}
