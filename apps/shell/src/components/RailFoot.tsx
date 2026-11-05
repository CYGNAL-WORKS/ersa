import { type MouseEvent, useEffect, useRef, useState } from "react";
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
  const problem = foot.kind === "reconnecting" || foot.kind === "unreachable";

  // Whether the foot has shown a problem since Ersa last reached the sessions, and whether it has just reached
  // them again after one: then assistive technology hears so, though nothing is shown.
  const [troubled, setTroubled] = useState(false);
  const [reconnected, setReconnected] = useState(false);
  if (problem && !troubled) {
    setTroubled(true);
    setReconnected(false);
  } else if (view.phase === "connected" && troubled) {
    setTroubled(false);
    setReconnected(true);
  }

  // Retry pressed from the keyboard: if it comes back, so does focus, rather than falling to the window.
  const retryButton = useRef<HTMLButtonElement>(null);
  const [refocus, setRefocus] = useState(false);
  if (refocus && view.phase === "connected") setRefocus(false);
  const retryShown = foot.kind === "unreachable";
  useEffect(() => {
    if (refocus && retryShown) {
      retryButton.current?.focus();
      setRefocus(false);
    }
  }, [refocus, retryShown]);
  const retry = (event: MouseEvent<HTMLButtonElement>) => {
    setRefocus(event.currentTarget.matches(":focus-visible"));
    onRetry();
  };

  return (
    <>
      {/* One status region for as long as the rail is: mounted empty, so a problem line arriving in it is announced. */}
      <div className={foot.kind === "reconnecting" ? "rail-foot" : "rail-foot-unseen"} role="status">
        {foot.kind === "reconnecting" ? (
          <>
            <p className="problem rail-foot-trying">{t("rail.foot.reconnectingError")}</p>
            {foot.reason && <p className="rail-foot-reason">{foot.reason}</p>}
          </>
        ) : (
          reconnected && t("rail.foot.reconnectedStatus")
        )}
      </div>
      {foot.kind === "unreachable" && (
        <div className="rail-foot" role="alert">
          <p className="problem">{t("rail.foot.unreachableError")}</p>
          {foot.reason && <p className="rail-foot-reason">{foot.reason}</p>}
          <Button ref={retryButton} onClick={retry}>
            {t("rail.foot.retryButton")}
          </Button>
        </div>
      )}
      {foot.kind === "attached" && <p className="rail-foot-attached">{t("rail.foot.attachedLabel", { label: foot.label })}</p>}
    </>
  );
}
