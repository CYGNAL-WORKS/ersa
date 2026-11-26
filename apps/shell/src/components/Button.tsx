import { LoaderCircle } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode, Ref } from "react";
import { Icon } from "./Icon";
import "./Button.css";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  /** Passed to the button element, for a parent that moves focus to it. */
  ref?: Ref<HTMLButtonElement>;
  /** For the gallery: show a pointer or keyboard state without one. */
  force?: "hover" | "focus" | "pressed";
  /** `primary` is the one action a moment asks for, such as Allow; quiet is everything else. */
  variant?: "quiet" | "primary";
};

export function Button({ force, variant = "quiet", className, ...rest }: ButtonProps) {
  return (
    <button
      type="button"
      {...rest}
      data-force={force}
      data-variant={variant}
      className={className ? `button ${className}` : "button"}
    />
  );
}

/**
 * Every label a button can take, held in one cell, so the button is as wide as the widest and nothing beside it moves
 * as its label changes (docs/design/DESIGN.md, Motion). Only `shown` is seen and heard. The `busy` label, while it
 * is shown, is led by the ring for work of unknown length, turning once per motion-loop-ring.
 */
export function ButtonLabels<K extends string>({ labels, shown, busy }: { labels: Record<K, ReactNode>; shown: K; busy?: K }) {
  return (
    <span className="button-labels">
      {(Object.entries(labels) as [K, ReactNode][]).map(([key, label]) => (
        <span key={key} className="button-label" data-shown={key === shown || undefined} aria-hidden={key !== shown || undefined}>
          {key === busy && (
            <span className="button-ring" aria-hidden="true">
              <Icon icon={LoaderCircle} size="icon-xs" />
            </span>
          )}
          {label}
        </span>
      ))}
    </span>
  );
}
