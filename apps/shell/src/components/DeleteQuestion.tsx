import { LoaderCircle } from "lucide-react";
import { type KeyboardEvent, type ReactNode, useEffect, useId, useLayoutEffect, useRef } from "react";
import { t } from "../catalog";
import { type DeletePhase, type WhatGoes, losesWork } from "../session/deletion";
import { Button } from "./Button";
import { Icon } from "./Icon";
import "./DeleteQuestion.css";

interface DeleteQuestionProps {
  /** The session's title as its tile shows it. */
  title: string;
  /** The backend's own name, such as Claude Code: the conversation goes from its tools too. */
  backend: string;
  phase: Exclude<DeletePhase, { kind: "deleted" }>;
  /** The user confirmed the question. */
  onConfirm: () => void;
  /** After a failure: ask again with what is left. */
  onTryAgain: () => void;
  /** Cancel, or Escape: everything stays as it was. */
  onCancel: () => void;
  /** For the gallery: drawn in place, taking no focus, rather than over the window. */
  inline?: boolean;
}

/**
 * The delete question (docs/design/RAIL.md, The delete question; FLOW-ARCHIVE-DELETE-SESSION, step 4): it names the
 * session, says its conversation goes for good, and says what goes with it or that its files stay. Focus starts on
 * Cancel and stays within it; while the deletion runs, both buttons and Escape wait for it to end.
 */
export function DeleteQuestion({ title, backend, phase, onConfirm, onTryAgain, onCancel, inline = false }: DeleteQuestionProps) {
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const confirm = useRef<HTMLButtonElement>(null);
  const deleting = phase.kind === "deleting";
  const failed = phase.kind === "failed";

  // Each phase places focus as it begins, so it never falls out of the dialog with a button that became unavailable.
  // Asking, and reading what to ask, it starts on Cancel, so an Enter pressed out of habit deletes nothing, however
  // many times Ersa asks; while the deletion runs both buttons are unavailable, so the dialog itself keeps it; a
  // failure gives it to Try again.
  useEffect(() => {
    if (inline) return;
    const to = phase.kind === "deleting" ? root : phase.kind === "failed" ? confirm : cancel;
    to.current?.focus({ preventScroll: true });
  }, [inline, phase]);

  // Focus stays within the dialog: a press anywhere else in the window, all of it inert while the question is open,
  // takes no focus, so Escape and Tab still reach the dialog.
  useEffect(() => {
    if (inline) return;
    const away = (event: MouseEvent) => {
      if (event.target instanceof Node && root.current?.contains(event.target)) return;
      event.preventDefault();
    };
    document.addEventListener("mousedown", away, true);
    return () => document.removeEventListener("mousedown", away, true);
  }, [inline]);
  // Centered as it opens, it then keeps its bottom edge where it was and grows upward as the counts arrive, so the
  // buttons never move under the pointer. Measured, not designed: centered again when the window is resized.
  useLayoutEffect(() => {
    const el = root.current;
    if (inline || !el) return;
    const anchor = () => {
      el.removeAttribute("data-anchored");
      const bottom = window.innerHeight - el.getBoundingClientRect().bottom;
      el.style.setProperty("--delete-question-bottom", `${bottom}px`);
      el.setAttribute("data-anchored", "");
    };
    anchor();
    window.addEventListener("resize", anchor);
    return () => window.removeEventListener("resize", anchor);
  }, [inline]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.stopPropagation();
      if (!deleting) onCancel();
    } else if (event.key === "Tab") {
      // Focus stays within the dialog: Tab from the last button wraps to the first, and Shift+Tab the other way.
      const buttons = [...(root.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
      const first = buttons[0];
      const last = buttons[buttons.length - 1];
      if (!first || !last) {
        event.preventDefault();
        return;
      }
      if (event.shiftKey && (document.activeElement === first || !root.current?.contains(document.activeElement))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (document.activeElement === last || !root.current?.contains(document.activeElement))) {
        event.preventDefault();
        first.focus();
      }
    }
  };

  const goes = phase.kind === "asking" || phase.kind === "deleting" ? phase.goes : undefined;
  // Every label the confirming button can take, and the one it shows now. All of them hold its width, so it keeps one
  // width from phase to phase and Cancel, beside it, never moves under the pointer.
  const labels = {
    delete: t("rail.delete.deleteButton"),
    lose: t("rail.delete.deleteAndLoseButton"),
    deleting: t("rail.delete.deletingProgress"),
    tryAgain: t("rail.delete.tryAgainButton"),
  };
  const shown: keyof typeof labels = failed ? "tryAgain" : deleting ? "deleting" : goes && losesWork(goes) ? "lose" : "delete";

  return (
    <div
      ref={root}
      className="delete-question"
      data-inline={inline || undefined}
      role="dialog"
      aria-modal={!inline || undefined}
      aria-labelledby={`${id}-title`}
      aria-describedby={`${id}-detail`}
      aria-busy={deleting || phase.kind === "reading" || undefined}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      <h2 className="delete-question-title" id={`${id}-title`}>
        {t("rail.delete.title", { title })}
      </h2>
      <div className="delete-question-detail" id={`${id}-detail`}>
        {phase.kind === "asking" && phase.changed && <p>{t("rail.delete.changedDetail")}</p>}
        <p>{t("rail.delete.conversationDetail", { backend })}</p>
        {goes && <Goes goes={goes} />}
        {phase.kind === "reading" && <p role="status">{t("rail.delete.readingProgress")}</p>}
      </div>
      {goes && <Loss goes={goes} />}
      {goes?.kind === "folderGoes" && goes.branchKept && <p className="delete-question-kept">{t("rail.delete.branchKeptDetail")}</p>}
      {failed && (
        <div className="delete-question-failed" role="alert">
          <p className="problem">{t("rail.delete.failedError")}</p>
          <p className="delete-question-reason">{phase.reason}</p>
        </div>
      )}
      <div className="delete-question-actions">
        <Button ref={cancel} onClick={onCancel} disabled={deleting}>
          {t("rail.delete.cancelButton")}
        </Button>
        <Button
          ref={confirm}
          className="delete-question-confirm"
          onClick={failed ? onTryAgain : onConfirm}
          disabled={deleting || phase.kind === "reading"}
        >
          {Object.entries(labels).map(([key, label]) => (
            <span key={key} className="delete-question-label" data-shown={key === shown || undefined} aria-hidden={key !== shown || undefined}>
              {key === "deleting" && (
                <span className="delete-question-ring" aria-hidden="true">
                  <Icon icon={LoaderCircle} size="icon-xs" />
                </span>
              )}
              {label}
            </span>
          ))}
        </Button>
      </div>
    </div>
  );
}

/** What goes with the conversation: the folder and its branch by name, or where the files stay. */
function Goes({ goes }: { goes: WhatGoes }) {
  if (goes.kind === "filesStay") return <p>{t("rail.delete.filesStayDetail", { path: <span className="delete-question-name">{goes.path}</span> })}</p>;
  if (goes.branch === "") return <p>{t("rail.delete.folderDetail")}</p>;
  const branch = <span className="delete-question-name">{goes.branch}</span>;
  return <p>{goes.branchKept ? t("rail.delete.folderBranchKeptDetail", { branch }) : t("rail.delete.folderAndBranchDetail", { branch })}</p>;
}

// The counts read as one list: "3 uncommitted or untracked files and 12 ignored files".
const list = new Intl.ListFormat("en-US", { type: "conjunction" });

/** What is lost with the folder and exists nowhere else, led by a caution glyph; nothing when nothing is. */
function Loss({ goes }: { goes: WhatGoes }) {
  if (goes.kind !== "folderGoes" || !losesWork(goes)) return null;
  const parts: string[] = [];
  if (goes.uncommitted > 0) parts.push(String(t("rail.delete.uncommittedPart", { count: goes.uncommitted })));
  if (goes.ignored > 0) parts.push(String(t("rail.delete.ignoredPart", { count: goes.ignored })));
  if (goes.stranded > 0) parts.push(String(t("rail.delete.strandedPart", { count: goes.stranded })));
  const said: ReactNode[] = [];
  if (parts.length > 0) said.push(t("rail.delete.lossDetail", { parts: list.format(parts) }));
  // Commits git can no longer count are named in place of a count of them (FLOW-ARCHIVE-DELETE-SESSION-R10).
  if (goes.unrecorded) said.push(t("rail.delete.unrecordedDetail"));
  return (
    <p className="delete-question-loss">
      <span className="glyph" data-tone="caution" aria-hidden="true" />
      <span>
        {said.map((s, i) => (
          <span key={i}>
            {i > 0 && " "}
            {s}
          </span>
        ))}
      </span>
    </p>
  );
}
