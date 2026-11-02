import { useEffect, useRef } from "react";
import { Button } from "./Button";
import "./TrustQuestion.css";

interface TrustQuestionProps {
  /** The project's name, as the question names it. */
  name: string;
  /** The folder as the user typed it, shown when the project is not added yet, so they see exactly what they trust. */
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

/** Asks whether the user trusts a project, saying plainly what trust lets run (ADR 0018). */
export function TrustQuestion({ name, path, purpose, onAnswer, onDismiss, takeFocus = true, forceCancelFocus }: TrustQuestionProps) {
  // The control that opened the question is disabled while it shows, so focus moves here rather than to the
  // page: to Cancel, so a second Enter cancels and never trusts by accident.
  const cancel = useRef<HTMLButtonElement>(null);
  // Only as it opens, so the first value of `takeFocus` is the one that counts.
  const focusOnOpen = useRef(takeFocus);
  useEffect(() => {
    if (focusOnOpen.current) cancel.current?.focus({ preventScroll: true });
  }, []);
  return (
    <section
      className="trust-question"
      aria-label={`Trust ${name}?`}
      onKeyDown={(event) => {
        if (event.key === "Escape") onDismiss();
      }}
    >
      <p className="trust-question-title">Trust {name}?</p>
      {path !== undefined && <p className="trust-question-path mono">{path}</p>}
      <p className="trust-question-detail">
        A trusted project’s own configuration runs in its sessions: hooks and MCP servers that start programs on this computer, and
        settings that change what the agent may do without asking.
      </p>
      <p className="trust-question-detail">
        Claude Code records the trust for the whole repository the project is in, and applies it outside Ersa too.
      </p>
      <p className="trust-question-detail">Untrusted, its sessions use only your own settings.</p>
      <div className="trust-question-actions">
        {/* Neither answer is the primary one: Ersa does not lean towards trust. */}
        {purpose === "add" ? (
          <>
            <Button onClick={() => onAnswer(true)}>Add, trusted</Button>
            <Button onClick={() => onAnswer(false)}>Add, not trusted</Button>
          </>
        ) : (
          <Button onClick={() => onAnswer(true)}>Trust</Button>
        )}
        <Button ref={cancel} onClick={onDismiss} force={forceCancelFocus ? "focus" : undefined}>
          Cancel
        </Button>
      </div>
    </section>
  );
}

/** A folder's last name, for a question about a project not yet added. */
export function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}
