import { Button } from "./Button";
import "./TrustQuestion.css";

interface TrustQuestionProps {
  /** The project's name, as the question names it. */
  name: string;
  /** The user's answer: whether the project's own configuration may run in its sessions. */
  onAnswer: (trusted: boolean) => void;
  /** While the answer is being recorded, so it cannot be given twice. */
  busy?: boolean;
}

/** Asks whether the user trusts a project, saying plainly what trust lets run (ADR 0018). */
export function TrustQuestion({ name, onAnswer, busy }: TrustQuestionProps) {
  return (
    <section className="trust-question" aria-label={`Trust ${name}?`}>
      <p className="trust-question-title">Trust {name}?</p>
      <p className="trust-question-detail">
        A trusted project’s own configuration runs in its sessions: hooks and MCP servers that start programs on this computer, and
        settings that change what the agent may do without asking.
      </p>
      <p className="trust-question-detail">Untrusted, its sessions use only your own settings.</p>
      <div className="trust-question-actions">
        {/* Neither answer is the primary one: Ersa does not lean towards trust. */}
        <Button onClick={() => onAnswer(true)} disabled={busy}>
          Trust
        </Button>
        <Button onClick={() => onAnswer(false)} disabled={busy}>
          Don’t trust
        </Button>
      </div>
    </section>
  );
}

/** A folder's last name, for a question about a project not yet added. */
export function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}
