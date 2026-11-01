import type { ButtonHTMLAttributes } from "react";
import "./Button.css";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
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
