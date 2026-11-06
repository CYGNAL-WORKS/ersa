import { Plus } from "lucide-react";
import { t } from "../catalog";
import { Button } from "./Button";
import { Icon } from "./Icon";
import "./RailTop.css";

/**
 * The rail's top row (docs/design/RAIL.md, The rail): New session, the button every other surface uses, filling the
 * row. The arrange button joins it at the trailing side when the arrange menu is built.
 */
export function RailTop({
  onNewSession,
  disabled,
  force,
}: {
  /** Opens the new-session screen, with no project chosen for it. */
  onNewSession: () => void;
  /** Ersa has not connected to Core yet. */
  disabled?: boolean;
  /** For the gallery: a pointer or keyboard state without one. */
  force?: "hover" | "focus" | "pressed";
}) {
  return (
    <div className="rail-top">
      <Button className="rail-new-session" onClick={onNewSession} disabled={disabled} force={force}>
        <Icon icon={Plus} />
        {t("rail.top.newSessionButton")}
      </Button>
    </div>
  );
}
