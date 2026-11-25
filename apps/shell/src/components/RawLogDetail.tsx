import { X } from "lucide-react";
import { type KeyboardEvent, useMemo, useState } from "react";
import { t } from "../catalog";
import type { RawLogEntry } from "../protocol/core-api.gen";
import { size } from "../session/attachments";
import { indentJson, lineBytes, lineText, rawLogTime } from "../session/rawLogText";
import { CopyButton } from "./CopyButton";
import { IconButton } from "./Icon";
import "./RawLogDetail.css";

/**
 * The detail beneath the developer pane's list (FLOW-READ-RAW-LOG, step 4; docs/design/WORKBENCH.md, The detail): the
 * selected line whole, indented when it is JSON (R10), or the start Core sent of a cut line, unchanged, with how large
 * the whole line is (R11). Copy puts on the clipboard what crossed, or what is shown of a cut line. It grows in from the
 * pane's foot and runs back the same way, keeping the line it showed while it goes.
 */
export function RawLogDetail({
  line: given,
  backend,
  onClose,
  force,
}: {
  /** The selected line; undefined while no row is selected. */
  line: RawLogEntry | undefined;
  backend: string;
  /** Close line, or Escape inside the detail: clears the selection, and the list takes keyboard focus. */
  onClose: () => void;
  /** For the gallery: Copy or Close line in a pointer or keyboard state, or Copy just after copying. */
  force?: { copy?: "hover" | "pressed" | "focus"; copied?: boolean; close?: "hover" | "pressed" | "focus" };
}) {
  // The line it showed stays while the detail runs back, so it leaves with its words rather than empty.
  const [kept, setKept] = useState(given);
  if (given !== undefined && given !== kept) setKept(given);
  const line = given ?? kept;
  const open = given !== undefined;

  const cut = line !== undefined && "head" in line;
  const text = line === undefined ? "" : lineText(line);
  // A whole line is shown indented when it is JSON; a cut one's start is shown as it came (R11).
  const shown = useMemo(() => (cut ? text : (indentJson(text) ?? text)), [cut, text]);
  const bytes = line === undefined ? 0 : "head" in line ? line.size : lineBytes(line.line);

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    onClose();
  };

  return (
    <section
      className="raw-log-detail"
      data-shown={open || undefined}
      // Out of reach and unheard while it runs back.
      inert={!open}
      aria-hidden={!open || undefined}
      aria-label={line && t(line.dir === "backend" ? "developer.fromBackendLabel" : "developer.toBackendLabel", { backend })}
      onKeyDown={onKeyDown}
    >
      {line && (
        <>
          <div className="raw-log-detail-head">
            <span>{t(line.dir === "backend" ? "developer.fromBackendLabel" : "developer.toBackendLabel", { backend })}</span>
            <span className="raw-log-detail-figure">{rawLogTime(line.ts)}</span>
            <span className="raw-log-detail-figure">{size(bytes)}</span>
            <span className="raw-log-detail-actions">
              <CopyButton text={text} label={t("developer.copyLineButton")} force={force?.copy} copied={force?.copied} />
              <IconButton
                icon={X}
                size="icon-sm"
                label={t("developer.closeLineButton")}
                className="raw-log-detail-close"
                force={force?.close}
                onClick={onClose}
              />
            </span>
          </div>
          <div className="raw-log-detail-body" tabIndex={0}>
            {cut && <p className="raw-log-detail-note">{t("developer.cutNote", { size: size(bytes) })}</p>}
            <pre className="raw-log-detail-text">{shown}</pre>
          </div>
        </>
      )}
    </section>
  );
}
