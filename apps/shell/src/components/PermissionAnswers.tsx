import { LoaderCircle } from "lucide-react";
import type { MouseEvent } from "react";
import { t } from "../catalog";
import type { PermissionRequested } from "../protocol/agent-events.gen";
import type { Answer, Decision, Reply } from "../session/answers";
import { Button } from "./Button";
import { Icon } from "./Icon";
import { Revealed } from "./Transcript";
import "./PermissionAnswers.css";

interface PermissionAnswersProps {
  request: PermissionRequested;
  /** The answer on its way, or refused, for this request. */
  answer?: Answer;
  /** Whether answers can reach Core now. */
  live: boolean;
  /** Says Ersa is reconnecting while it can't reach Core, unless the surface around it already does. */
  reconnectingNote?: boolean;
  onDecide: (decision: Decision) => void;
}

/**
 * A permission request's answers, the ones the request carries (FLOW-APPROVE-PERMISSION-R12): one per suggestion,
 * then Deny and Allow, with Allow the primary unless the request defaults to no (R14). The shelf and the
 * transcript's permission card both offer them so (R27). docs/design/RAIL.md, The shelf: the answers, notes and
 * unavailable answers.
 */
export function PermissionAnswers({ request, answer, live, reconnectingNote = true, onDecide }: PermissionAnswersProps) {
  const denyFirst = request.flags.default_to_no;
  const suggestions = request.flags.suppress_always_allow ? [] : request.suggestions;
  const sending = answer?.state === "sending" ? answer.decision : undefined;
  // One answer at a time, and none while Core can't be reached (R18, R20).
  const unavailable = sending !== undefined || !live;
  // Choosing an answer makes every answer unavailable, so focus moves to the surface around them (the shelf, or
  // the transcript's card, each focusable outside the tab order) rather than fall to the page with the button.
  const choose = (decision: Decision) => (event: MouseEvent<HTMLButtonElement>) => {
    const holder = event.currentTarget.closest<HTMLElement>('[tabindex="-1"]');
    onDecide(decision);
    holder?.focus();
  };
  // The answer on its way leads its label with a ring.
  const ring = (decision: Decision) =>
    sending !== undefined &&
    same(sending, decision) && (
      <span className="answers-ring" aria-hidden="true">
        <Icon icon={LoaderCircle} size="icon-xs" />
      </span>
    );
  return (
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
      {suggestions.map((s, i) => (
        <Button key={i} className="answers-suggestion" disabled={unavailable} onClick={choose({ suggestion: i })}>
          {ring({ suggestion: i })}
          <Revealed text={s.label} />
        </Button>
      ))}
      <div className="answers-row">
        <Button variant={denyFirst ? "primary" : "quiet"} disabled={unavailable} onClick={choose("deny")}>
          {ring("deny")}
          {t("rail.shelf.denyButton")}
        </Button>
        <Button variant={denyFirst ? "quiet" : "primary"} disabled={unavailable} onClick={choose("allow")}>
          {ring("allow")}
          {t("rail.shelf.allowButton")}
        </Button>
      </div>
    </div>
  );
}

function same(a: Decision | Reply, b: Decision): boolean {
  if (typeof a === "object" && typeof b === "object") return "suggestion" in a && a.suggestion === b.suggestion;
  return a === b;
}
