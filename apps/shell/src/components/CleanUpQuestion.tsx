import { useRef } from "react";
import { t } from "../catalog";
import { size } from "../session/attachments";
import { lostParts, questionOf, questionTitle } from "../session/cleanup";
import type { RunView } from "../session/cleanupList";
import { Button, ButtonLabels } from "./Button";
import { Dialog, DialogLoss } from "./Dialog";

/**
 * The clean-up question (FLOW-CLEAN-UP, steps 6 and 7; R17, R18, R28, R29; docs/design/SETTINGS.md, The clean-up
 * question): it names how many folders and branches go and the space freed, and what is lost for good; nothing is
 * removed until it is confirmed. While the items are removed it stays, saying how many are done, and its confirming
 * button becomes Stop, which lets the item under way finish.
 */
export function CleanUpQuestion({
  run,
  onConfirm,
  onCancel,
  onStop,
  inline = false,
  forceConfirm,
}: {
  run: RunView;
  onConfirm: () => void;
  onCancel: () => void;
  onStop: () => void;
  /** For the gallery: drawn in place, taking no focus, and the confirming button's keyboard focus shown. */
  inline?: boolean;
  forceConfirm?: "focus";
}) {
  const cancel = useRef<HTMLButtonElement>(null);
  const confirm = useRef<HTMLButtonElement>(null);
  const removing = run.state === "removing";
  const stopping = removing && run.stopping;
  const q = questionOf(run.ticked);
  const lost = lostParts(q);

  // Every label the confirming button takes, all holding its width, so Cancel beside it never moves (DESIGN.md).
  const labels = {
    confirm: t("settings.storage.cleanUp.confirmButton"),
    discard: t("settings.storage.cleanUp.discardButton"),
    stop: t("settings.storage.cleanUp.stopButton"),
    stopping: t("settings.storage.cleanUp.stoppingProgress"),
  };
  const shown: keyof typeof labels = stopping ? "stopping" : removing ? "stop" : q.discards ? "discard" : "confirm";

  return (
    <Dialog
      title={questionTitle(q)}
      busy={removing}
      onCancel={() => {
        if (!removing) onCancel();
      }}
      // Asking, focus starts on Cancel, so an Enter pressed out of habit removes nothing; removing, it is on Stop, which
      // keeps it while it reads Stopping….
      focusTo={removing ? confirm : cancel}
      focusKey={run.state}
      inline={inline}
      detail={
        <>
          {q.freesSpace && <p>{t("settings.storage.cleanUp.freesDetail", { size: size(q.bytes) })}</p>}
          {q.sessions && <p>{t("settings.storage.cleanUp.keepsDetail")}</p>}
          {removing && (
            <p role="status">{t("settings.storage.cleanUp.removedProgress", { done: run.done, count: run.ticked.length })}</p>
          )}
        </>
      }
      actions={
        <>
          <Button ref={cancel} onClick={onCancel} disabled={removing}>
            {t("settings.storage.cleanUp.cancelButton")}
          </Button>
          <Button
            ref={confirm}
            className={removing ? undefined : "dialog-confirm"}
            onClick={removing ? onStop : onConfirm}
            // Stopping, it keeps focus and says it is busy; a second press would change nothing.
            aria-disabled={stopping || undefined}
            force={forceConfirm}
          >
            <ButtonLabels labels={labels} shown={shown} busy="stopping" />
          </Button>
        </>
      }
    >
      {lost && <DialogLoss>{t("settings.storage.cleanUp.lostDetail", { parts: lost })}</DialogLoss>}
      {q.unrecorded > 0 && <DialogLoss>{t("settings.storage.cleanUp.unrecordedQuestion", { count: q.unrecorded })}</DialogLoss>}
      {q.repositories > 0 && <DialogLoss>{t("settings.storage.cleanUp.repositoriesDetail", { count: q.repositories })}</DialogLoss>}
      {q.strays > 0 && <DialogLoss>{t("settings.storage.cleanUp.folderQuestion", { count: q.strays })}</DialogLoss>}
      {q.unfinished > 0 && <DialogLoss>{t("settings.storage.cleanUp.unfinishedQuestion", { count: q.unfinished })}</DialogLoss>}
    </Dialog>
  );
}
