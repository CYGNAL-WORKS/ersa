import { t } from "../catalog";
import type { CoreView } from "../core/connection";
import { footOf } from "../core/foot";
import { Button } from "./Button";
import "./RailFoot.css";

interface RailFootProps {
  view: CoreView;
  /** Attach again, after Ersa has stopped trying. A lost connection reattaches by itself. */
  onRetry: () => void;
}

/** The rail's foot: nothing while all is well, else what is wrong in the user's terms (LAY-SHELL, The foot). */
export function RailFoot({ view, onRetry }: RailFootProps) {
  const foot = footOf(view);
  switch (foot.kind) {
    case "silent":
      return null;
    case "attached":
      return <p className="rail-foot-attached">{t("rail.foot.attachedLabel", { label: foot.label })}</p>;
    case "reconnecting":
      // A status region: assistive technology reads it politely when it appears.
      return (
        <div className="rail-foot" role="status">
          <p className="problem rail-foot-trying">{t("rail.foot.reconnectingError")}</p>
          {foot.reason && <p className="rail-foot-reason">{foot.reason}</p>}
        </div>
      );
    case "unreachable":
      return (
        <div className="rail-foot" role="alert">
          <p className="problem">{t("rail.foot.unreachableError")}</p>
          {foot.reason && <p className="rail-foot-reason">{foot.reason}</p>}
          <Button onClick={onRetry}>{t("rail.foot.retryButton")}</Button>
        </div>
      );
  }
}
