import { Archive } from "lucide-react";
import { t } from "../catalog";
import type { Sending } from "../session/follow";
import { Button } from "./Button";
import { Icon } from "./Icon";
import "./ArchivedNote.css";

/**
 * What an archived session shows in place of the composer (FLOW-ARCHIVE-DELETE-SESSION, step 2): a note that it is
 * archived, with Resume, which unarchives and resumes it. Its transcript reads as usual above it. While its Resume
 * waits for the session's folder to be put back, the note says so in place of the archived line (FLOW-CLEAN-UP-R21).
 */
export function ArchivedNote({
  onResume,
  resuming,
  force,
}: {
  onResume: () => void;
  /** A resume on its way, with how far it has got, or why Core refused the last. */
  resuming?: Sending;
  /** For the gallery: a pointer or keyboard state on Resume without one. */
  force?: "hover" | "focus" | "pressed";
}) {
  const busy = resuming?.state === "sending";
  const puttingBack = busy && resuming.progress?.phase === "puttingBack";
  const words = t("composer.puttingBackProgress");
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
        {/* Both lines in one place, the one not shown faded out, so they crossfade and the row keeps its size. */}
        <p className="archived-words">
          <span className="archived-line" data-shown={!puttingBack || undefined} aria-hidden={puttingBack || undefined}>
            {t("composer.archived.note")}
          </span>
          <span className="archived-line" data-shown={puttingBack || undefined} aria-hidden>
            {words}
          </span>
        </p>
        {/* Mounted with the note, so the words arriving in it are announced, as the status line's are. */}
        <span className="heard-only" role="status">
          {puttingBack ? words : ""}
        </span>
        {/* Unavailable while Core answers, yet still focusable, so keyboard focus stays on it through a refusal. */}
        <Button variant="primary" aria-disabled={busy || undefined} onClick={() => !busy && onResume()} force={force}>
          {busy ? t("composer.archived.resumingProgress") : t("composer.archived.resumeButton")}
        </Button>
      </div>
    </div>
  );
}
