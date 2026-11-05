import type { LucideIcon } from "lucide-react";
import type { ButtonHTMLAttributes, Ref } from "react";
import "./Icon.css";

/** The icon sizes DESIGN.md names: beside `ui` type, beside `ui-small` type, and a mark inside a tile. */
export type IconSize = "icon" | "icon-sm" | "icon-xs";

/**
 * An icon (DESIGN.md, Iconography): a Lucide icon, even strokes with round ends, sized by token and drawn in the
 * ink of the text around it. It sits beside a label, so it is hidden from assistive technology; an icon that
 * stands alone goes in an `IconButton`, which names it.
 */
export function Icon({ icon: Glyph, size = "icon" }: { icon: LucideIcon; size?: IconSize }) {
  // The size reaches the element through CSS, from the token, so a changed token changes every icon. The stroke
  // is Lucide's own, 2 units on its 24-unit grid, and scales with the icon (DESIGN.md, Iconography).
  return <Glyph className="icon" data-size={size} aria-hidden="true" />;
}

type IconButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children" | "aria-label" | "title"> & {
  ref?: Ref<HTMLButtonElement>;
  icon: LucideIcon;
  size?: IconSize;
  /** What the button does: its accessible name and its tooltip. An icon without a visible label always has one. */
  label: string;
  /** For the gallery: show a pointer or keyboard state without one. */
  force?: "hover" | "focus" | "pressed";
};

/** A square button holding only an icon, in `ink-muted` and reading `ink` under the pointer. */
export function IconButton({ icon, size, label, force, className, ...rest }: IconButtonProps) {
  return (
    <button
      type="button"
      {...rest}
      aria-label={label}
      title={label}
      data-force={force}
      className={className ? `icon-button ${className}` : "icon-button"}
    >
      <Icon icon={icon} size={size} />
    </button>
  );
}
