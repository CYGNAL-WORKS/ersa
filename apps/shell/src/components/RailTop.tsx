import { useId } from "react";
import { Plus } from "lucide-react";
import { t } from "../catalog";
import { type ArrangeActions, ArrangeButton } from "./ArrangeMenu";
import { Button } from "./Button";
import type { ArrangeChoices } from "../session/arrangement";
import { Icon } from "./Icon";
import "./RailTop.css";

/**
 * The rail's top row (docs/design/RAIL.md, The rail): New session, the button every other surface uses, filling the
 * row but for the arrange button at its trailing side.
 */
export function RailTop({
  onNewSession,
  disabled,
  force,
  arrange,
}: {
  /** The arrange button: the arrangement its menu names, and what its items do. */
  arrange: ArrangeActions & {
    arrangement: ArrangeChoices;
    /** The arrangement has not been read yet. */
    reading?: boolean;
    /** For the gallery: the menu open, with a submenu open by index. */
    initialOpen?: boolean;
    initialSubmenu?: number;
  };
  /** Opens the new-session screen, with no project chosen for it. */
  onNewSession: () => void;
  /** Ersa has not connected to Core yet, or has lost it: New session keeps focus and says why it can't be used. */
  disabled?: boolean;
  /** For the gallery: a pointer or keyboard state without one. */
  force?: "hover" | "focus" | "pressed";
}) {
  const reason = useId();
  return (
    <div className="rail-top">
      {/* Unavailable but never disabled, which would drop keyboard focus: it keeps focus and says why, as an item
          does, while the foot says what is wrong (LAY-SHELL, The foot). */}
      <Button
        className="rail-new-session"
        onClick={() => !disabled && onNewSession()}
        aria-disabled={disabled || undefined}
        aria-describedby={disabled ? reason : undefined}
        force={force}
      >
        <Icon icon={Plus} />
        {t("rail.top.newSessionButton")}
      </Button>
      {/* Read as its description, not shown: the foot shows what is wrong. */}
      {disabled && (
        <span id={reason} hidden>
          {t("rail.tile.offlineReason")}
        </span>
      )}
      {/* Unavailable, as New session is, until Ersa has connected (LAY-SHELL, The foot). */}
      <ArrangeButton {...arrange} disabled={disabled || arrange.reading} />
    </div>
  );
}
