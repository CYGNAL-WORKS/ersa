import { File, Folder, Image } from "lucide-react";
import { type CSSProperties, type ReactNode, useEffect, useId, useRef, useState } from "react";
import { Icon } from "./Icon";
import "./AttachmentChip.css";

const KIND_ICON = { file: File, image: Image, folder: Folder } as const;

interface AttachmentChipProps {
  kind: keyof typeof KIND_ICON;
  name: string;
  /** Ersa can't send it: the danger glyph in place of its icon (FLOW-SEND-MESSAGE-R10). */
  problem?: boolean;
  /**
   * The path the agent was given, as the transcript shows a file or folder (DESIGN.md, Layout): the chip takes keyboard
   * focus, names itself by the path, and shows it as a tooltip while the pointer rests on it, while it has keyboard
   * focus, and after a press on touch until the next press anywhere.
   */
  path?: string;
  /** The surface's own class, beside the chip's. */
  className?: string;
  /** What follows the name: the composer's detail and remove. */
  children?: ReactNode;
  /** For the gallery: the chip under the pointer, pressed, or with keyboard focus, its tooltip shown with focus. */
  force?: "hover" | "pressed" | "focus";
}

/**
 * One attachment as a chip (docs/design/COMPOSER.md, Attachments): its icon, then its name cut with an ellipsis, then
 * whatever the surface adds. The composer's attachments and the transcript's (FLOW-SEND-MESSAGE-R11) share it.
 */
export function AttachmentChip({ kind, name, problem, path, className, children, force }: AttachmentChipProps) {
  const classes = className ? `attachment ${className}` : "attachment";
  const parts = (
    <>
      {problem ? (
        <span className="glyph" data-tone="danger" aria-hidden="true" />
      ) : (
        <span className="attachment-icon">
          <Icon icon={KIND_ICON[kind]} size="icon-sm" />
        </span>
      )}
      <span className="attachment-name">{name}</span>
      {children}
    </>
  );
  if (path === undefined) {
    return (
      <li className={classes} data-problem={problem || undefined}>
        {parts}
      </li>
    );
  }
  return (
    <li className="attachment-item">
      <PathChip className={classes} path={path} force={force}>
        {parts}
      </PathChip>
    </li>
  );
}

/** The focusable chip of a file or folder in the transcript, and its tooltip. */
function PathChip({ className, path, force, children }: { className: string; path: string; force?: "hover" | "pressed" | "focus"; children: ReactNode }) {
  const id = useId();
  const anchor = `--attachment-${id.replace(/[^a-zA-Z0-9]/g, "")}`;
  const chip = useRef<HTMLButtonElement>(null);
  // Shown after a press on touch, where there is no resting pointer, until the next press anywhere.
  const [pressed, setPressed] = useState(false);
  useEffect(() => {
    if (!pressed) return;
    const close = (event: PointerEvent) => {
      if (event.target !== chip.current && !chip.current?.contains(event.target as Node)) setPressed(false);
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, [pressed]);
  return (
    <>
      <button
        ref={chip}
        type="button"
        className={`${className} attachment-path`}
        aria-label={path}
        data-force={force}
        data-tip={pressed || undefined}
        style={{ anchorName: anchor } as CSSProperties}
        onPointerDown={(event) => {
          if (event.pointerType === "touch") setPressed((was) => !was);
        }}
      >
        {children}
      </button>
      <span className="attachment-tip" aria-hidden="true" style={{ positionAnchor: anchor } as CSSProperties}>
        {path}
      </span>
    </>
  );
}
