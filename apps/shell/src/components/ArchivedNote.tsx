import { Archive } from "lucide-react";
import { t } from "../catalog";
import type { Sending } from "../session/follow";
import { Button } from "./Button";
import { Icon } from "./Icon";
import "./ArchivedNote.css";

/**
 * What an archived session shows in place of the composer (FLOW-ARCHIVE-DELETE-SESSION, step 2): a note that it is
 * archived, with Resume, which unarchives and resumes it. Its transcript reads as usual above it.
 */
export function ArchivedNote({
  onResume,
  resuming,
  force,
}: {
  onResume: () => void;
  /** A resume on its way, or why Core refused the last. */
  resuming?: Sending;
  /** For the gallery: a pointer or keyboard state on Resume without one. */
  force?: "hover" | "focus" | "pressed";
}) {
  const busy = resuming?.state === "sending";
  return (
    <div className="archived">
      {/* A refused resume leaves the session archived and says why, where the composer's status line would. */}
      {resuming?.state === "failed" && (
        <p className="problem archived-problem" role="alert">
          {t("composer.archived.resumeError")} {resuming.message}
        </p>
      )}
      <div className="archived-note">
        <span className="archived-icon">
          <Icon icon={Archive} size="icon-sm" />
        </span>
        <p className="archived-words">{t("composer.archived.note")}</p>
        <Button variant="primary" onClick={onResume} disabled={busy} force={force}>
          {busy ? t("composer.archived.resumingProgress") : t("composer.archived.resumeButton")}
        </Button>
      </div>
    </div>
  );
}
