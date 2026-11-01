import type { ButtonHTMLAttributes } from "react";
import "./Button.css";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  /** For the gallery: show a pointer or keyboard state without one. */
  force?: "hover" | "focus" | "pressed";
};

export function Button({ force, className, ...rest }: ButtonProps) {
  return <button type="button" {...rest} data-force={force} className={className ? `button ${className}` : "button"} />;
}
