import { type MouseEvent, useCallback, useEffect, useRef, useState } from "react";
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

  // Retry pressed from the keyboard: when a new Retry button appears after the attempt, focus goes back to it,
  // rather than falling to the window, unless the user has put focus somewhere else meanwhile.
  const refocus = useRef(false);
  useEffect(() => {
    if (view.phase === "connected") refocus.current = false;
  }, [view.phase]);
  // Stable, so it runs only as a Retry button mounts, not on each render of the one already there.
  const retryMounted = useCallback((button: HTMLButtonElement | null) => {
    if (!button || !refocus.current) return;
    refocus.current = false;
    if (document.activeElement === null || document.activeElement === document.body) button.focus();
  }, []);
  const retry = (event: MouseEvent<HTMLButtonElement>) => {
    refocus.current = event.currentTarget.matches(":focus-visible");
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
          <Button ref={retryMounted} onClick={retry}>
            {t("rail.foot.retryButton")}
          </Button>
        </div>
      )}
      {foot.kind === "attached" && <p className="rail-foot-attached">{t("rail.foot.attachedLabel", { label: foot.label })}</p>}
    </>
  );
}
