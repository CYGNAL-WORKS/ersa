import { File, Folder, Image } from "lucide-react";
import type { ReactNode } from "react";
import { Icon } from "./Icon";
import "./AttachmentChip.css";

const KIND_ICON = { file: File, image: Image, folder: Folder } as const;

interface AttachmentChipProps {
  kind: keyof typeof KIND_ICON;
  name: string;
  /** Ersa can't send it: the danger glyph in place of its icon (FLOW-SEND-MESSAGE-R10). */
  problem?: boolean;
  /** Said on hover, as the path the agent was given. */
  title?: string;
  /** The surface's own class, beside the chip's. */
  className?: string;
  /** What follows the name: the composer's detail and remove. */
  children?: ReactNode;
}

/**
 * One attachment as a chip (docs/design/COMPOSER.md, Attachments): its icon, then its name cut with an ellipsis, then
 * whatever the surface adds. The composer's attachments and the transcript's (FLOW-SEND-MESSAGE-R11) share it.
 */
export function AttachmentChip({ kind, name, problem, title, className, children }: AttachmentChipProps) {
  return (
    <li className={className ? `attachment ${className}` : "attachment"} title={title} data-problem={problem || undefined}>
      {problem ? (
        <span className="glyph" data-tone="danger" aria-hidden="true" />
      ) : (
        <span className="attachment-icon">
          <Icon icon={KIND_ICON[kind]} size="icon-sm" />
        </span>
      )}
      <span className="attachment-name">{name}</span>
      {children}
    </li>
  );
}
