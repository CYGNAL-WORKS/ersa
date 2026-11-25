import { Check, Copy } from "lucide-react";
import { useState } from "react";
import { t } from "../catalog";
import { Icon } from "./Icon";
import "./CopyButton.css";

/**
 * Copy (docs/design/TRANSCRIPT.md, Formatted text): a quiet icon button that puts `text` on the clipboard, then shows
 * a check and says "Copied" until the pointer leaves it or focus moves. A code block's head and the developer pane's
 * detail both use it.
 */
export function CopyButton({
  text,
  label,
  force,
  copied: forceCopied,
}: {
  /** Exactly what goes on the clipboard. */
  text: string;
  /** Its accessible name and tooltip before copying, such as "Copy code". */
  label: string;
  /** For the gallery: under the pointer, pressed, or with keyboard focus, without one. */
  force?: "hover" | "focus" | "pressed";
  /** For the gallery: as it is just after copying. */
  copied?: boolean;
}) {
  const [copiedNow, setCopied] = useState(false);
  const copied = copiedNow || forceCopied === true;
  const name = copied ? t("transcript.markdown.copiedLabel") : label;
  const copy = () => {
    // A refused write leaves Copy as it was: it says "Copied" only once the text is on the clipboard.
    navigator.clipboard.writeText(text).then(
      () => setCopied(true),
      () => {},
    );
  };
  const reset = () => setCopied(false);
  return (
    <button
      type="button"
      className="icon-button copy-button"
      aria-label={name}
      title={name}
      data-force={force}
      data-copied={copied || undefined}
      onClick={copy}
      onPointerLeave={reset}
      onBlur={reset}
    >
      {/* Both icons are drawn, one over the other, so the change between them can crossfade. */}
      <span className="copy-button-icons">
        <Icon icon={Copy} size="icon-sm" />
        <Icon icon={Check} size="icon-sm" />
      </span>
    </button>
  );
}
