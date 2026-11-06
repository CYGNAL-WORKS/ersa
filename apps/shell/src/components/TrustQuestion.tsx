import { useEffect, useRef } from "react";
import { t } from "../catalog";
import { Button } from "./Button";
import "./TrustQuestion.css";

interface TrustQuestionProps {
  /** The project's name, as the question names it. */
  name: string;
  /** The folder's full path, so the user sees exactly what they trust (FLOW-NEW-SESSION-R24). */
  path: string;
  /** Where it opens: above the composer's box when a project is added, in the rail when one is trusted later. */
  purpose: "add" | "change";
  /** Trust: the project is trusted (FLOW-NEW-SESSION-R25). */
  onTrust: () => void;
  /** Closes the question without trusting: Cancel, Enter while Cancel has focus, or Escape. */
  onDismiss: () => void;
  /** Whether the question takes focus as it opens; true when the user opened it, false for a question shown from the start. */
  takeFocus?: boolean;
  /** For the gallery: Cancel shown focused, as the keyboard leaves it when the question opens. */
  forceCancelFocus?: boolean;
}

/**
 * Asks whether the user trusts a folder, as the Claude Code CLI's folder-trust prompt does (ADR 0028): its path, the
 * safety check, and what agents can do there (FLOW-NEW-SESSION-R24), answered by Cancel, first and focused, or Trust
 * (R25). Trusting is never the default, so neither answer is primary.
 */
export function TrustQuestion({ name, path, purpose, onTrust, onDismiss, takeFocus = true, forceCancelFocus }: TrustQuestionProps) {
  // The control that opened the question is disabled or elsewhere while it shows, so focus moves here rather than
  // to the page: to Cancel, so Enter cancels until the user moves to Trust.
  const cancel = useRef<HTMLButtonElement>(null);
  const trust = useRef<HTMLButtonElement>(null);
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
      // The trust question); trusting an added project opens in the rail, under the project.
      data-place={purpose === "add" ? "composer" : "rail"}
      aria-label={title}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          // The question is the thing Escape closes, not whatever holds it.
          event.stopPropagation();
          onDismiss();
        } else if (
          ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key) &&
          (event.target === cancel.current || event.target === trust.current)
        ) {
          // On an answer, the arrows move between the two, as Tab does; elsewhere, such as the explanation when it
          // scrolls, they keep their own meaning.
          event.preventDefault();
          (document.activeElement === cancel.current ? trust : cancel).current?.focus({ preventScroll: true });
        }
      }}
    >
      <p className="trust-question-title">{title}</p>
      <p className="trust-question-path mono">{path}</p>
      {/* The explanation scrolls when the question has less room than it needs; the title, the folder and the
          answers above and below it always show. */}
      <div className="trust-question-body">
        <p className="trust-question-detail">{t("trust.question.safetyDetail")}</p>
        <p className="trust-question-detail">{t("trust.question.agentsDetail")}</p>
      </div>
      <div className="trust-question-actions">
        <Button ref={cancel} onClick={onDismiss} force={forceCancelFocus ? "focus" : undefined}>
          {t("trust.question.cancelButton")}
        </Button>
        <Button ref={trust} onClick={onTrust}>
          {t("trust.question.trustButton")}
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
