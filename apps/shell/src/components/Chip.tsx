import { type LucideIcon, ShieldAlert } from "lucide-react";
import { type KeyboardEvent, useEffect, useId, useRef, useState } from "react";
import { Icon } from "./Icon";
import "./Chip.css";

export interface ChipOption {
  id: string;
  label: string;
  /** A status shown beside the label, such as a backend that is missing. */
  status?: string;
  /** The status is a failure, so it takes the danger glyph; a plain status, such as not installed, takes none. */
  failed?: boolean;
  /** Shown with its status but not choosable: a backend that is missing or failed detection. */
  unavailable?: boolean;
  /** A second line under the label, such as a project's folder, cut at its start so its end stays in view. */
  detail?: string;
  /** A warning before the status, behind a shield in the caution tone: "Not trusted". */
  caution?: string;
}

/** The action that ends a chip's list, below a hairline: Add project…. */
export interface ChipAction {
  label: string;
  icon: LucideIcon;
  /** The keys that do the same from anywhere on the screen, shown at the trailing edge. */
  shortcut?: string;
  /** Absent while the action is shown but cannot be taken. */
  onChoose?: () => void;
}

interface ChipProps {
  /** What the chip sets, for the accessible name: "Workspace". */
  name: string;
  /** A muted word before the value, where the value alone would not say what it is: "Effort". */
  prefix?: string;
  /** The chosen option's id; absent when nothing can be chosen. */
  value?: string;
  /** Shown in place of a value when `value` is absent: "No backend". */
  placeholder?: string;
  /** The value reads as a problem: the danger glyph before it. */
  problem?: boolean;
  /** The value is still being found out: the pending glyph before it. */
  pending?: boolean;
  options: ChipOption[];
  onChoose: (id: string) => void;
  /** Ends the list. */
  action?: ChipAction;
  /** The list is `size-menu` wide, for options with a second line. */
  wide?: boolean;
  disabled?: boolean;
  /** For the gallery: the choices shown open, and a pointer or keyboard state shown without one. */
  initialOpen?: boolean;
  force?: "hover" | "focus" | "pressed";
  /** For the gallery: one option of an open list shown hovered or pressed. */
  forceOption?: { id: string; state: "hover" | "pressed" };
}

/**
 * One of the session controls: a pill naming a value, which opens its choices beneath it. The keyboard opens it
 * with Enter, Space or the arrows, moves with the arrows, chooses with Enter or Space, and Escape closes it with
 * focus back on the chip.
 */
export function Chip({ name, prefix, value, placeholder, problem, pending, options, onChoose, action, wide, disabled, initialOpen = false, force, forceOption }: ChipProps) {
  const [open, setOpen] = useState(initialOpen);
  const chip = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const listId = useId();
  const chosen = options.find((o) => o.id === value);
  const shown = chosen?.label ?? placeholder ?? "";

  // Focus the chosen option as the list opens from the keyboard or a click, so the arrows start from it.
  // The gallery's open chip does not take focus.
  const focusOnOpen = useRef(false);
  useEffect(() => {
    if (!open || !focusOnOpen.current) return;
    focusOnOpen.current = false;
    const items = optionsIn(list.current);
    (items.find((el) => el.dataset.id === value) ?? items[0])?.focus({ preventScroll: true });
  }, [open, value]);

  // A press anywhere outside closes the list, leaving focus where the press put it.
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);

  // Disabled while open, as when the first message is sent: the list goes too, so nothing changes after.
  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  const show = () => {
    focusOnOpen.current = true;
    setOpen(true);
  };
  // An arrow on the chip while its list is already open goes into the list at once.
  const enter = () => {
    const items = optionsIn(list.current);
    (items.find((el) => el.dataset.id === value) ?? items[0])?.focus({ preventScroll: true });
  };
  const close = () => {
    setOpen(false);
    chip.current?.focus({ preventScroll: true });
  };
  const choose = (option: ChipOption) => {
    if (option.unavailable || disabled) return;
    if (option.id !== value) onChoose(option.id);
    close();
  };
  const take = () => {
    if (!action?.onChoose || disabled) return;
    close();
    action.onChoose();
  };

  const onChipKey = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (open) enter();
      else show();
    } else if (event.key === "Escape" && open) {
      event.preventDefault();
      event.stopPropagation();
      setOpen(false);
    }
  };

  const onListKey = (event: KeyboardEvent<HTMLUListElement>) => {
    const items = optionsIn(list.current);
    const at = items.indexOf(document.activeElement as HTMLElement);
    const move = (to: number) => {
      event.preventDefault();
      items[(to + items.length) % items.length]?.focus({ preventScroll: true });
    };
    switch (event.key) {
      case "ArrowDown":
        return move(at + 1);
      case "ArrowUp":
        return move(at - 1);
      case "Home":
        return move(0);
      case "End":
        return move(items.length - 1);
      case "Escape":
        event.preventDefault();
        // The list is the chip's; Escape here is not also a request to leave the screen.
        event.stopPropagation();
        return close();
      case "Tab":
        // Tab moves on from the chip, as if the list had not been open.
        setOpen(false);
        return;
      case "Enter":
      case " ": {
        event.preventDefault();
        if (items[at]?.dataset.action !== undefined) return take();
        const option = options.find((o) => o.id === items[at]?.dataset.id);
        if (option) choose(option);
      }
    }
  };

  return (
    <div className="chip-root" ref={root}>
      <button
        ref={chip}
        type="button"
        className="chip"
        aria-label={`${name}: ${shown}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        data-force={force}
        data-open={open || undefined}
        data-pending={pending || undefined}
        disabled={disabled}
        onClick={() => (open ? setOpen(false) : show())}
        onKeyDown={onChipKey}
      >
        {problem && <span className="glyph" data-tone="danger" aria-hidden="true" />}
        {pending && <span className="glyph" data-tone="pending" aria-hidden="true" />}
        {prefix && <span className="chip-prefix">{prefix}</span>}
        <span className="chip-value">{shown}</span>
      </button>
      {open && (
        <ul ref={list} id={listId} className="chip-list" data-wide={wide || undefined} role="listbox" aria-label={name} onKeyDown={onListKey}>
          {options.map((option) => (
            <li
              key={option.id}
              role="option"
              tabIndex={-1}
              className="chip-option"
              data-id={option.id}
              data-two-line={option.detail !== undefined || undefined}
              data-force={forceOption?.id === option.id ? forceOption.state : undefined}
              aria-selected={option.id === value}
              aria-disabled={option.unavailable || undefined}
              onClick={() => choose(option)}
            >
              {/* The chosen option carries a mark in a slot every option keeps, so the labels align. */}
              <span className="chip-option-mark" aria-hidden="true" />
              <span className="chip-option-label">{option.label}</span>
              {option.caution && (
                <span className="chip-option-caution">
                  <span className="chip-option-shield">
                    <Icon icon={ShieldAlert} size="icon-sm" />
                  </span>
                  {option.caution}
                </span>
              )}
              {option.status && (
                <span className="chip-option-status">
                  {option.failed && <span className="glyph" data-tone="danger" aria-hidden="true" />}
                  {option.status}
                </span>
              )}
              {option.detail !== undefined && (
                // Laid out right to left so the ellipsis cuts the path's start; the path itself reads left to right.
                <span className="chip-option-detail">
                  <bdi dir="ltr">{option.detail}</bdi>
                </span>
              )}
            </li>
          ))}
          {action && (
            <>
              <li className="chip-list-rule" role="presentation" />
              <li
                role="option"
                tabIndex={-1}
                className="chip-option chip-action"
                data-action=""
                aria-selected={false}
                aria-disabled={action.onChoose === undefined || undefined}
                onClick={take}
              >
                <span className="chip-action-icon">
                  <Icon icon={action.icon} size="icon-sm" />
                </span>
                <span className="chip-option-label">{action.label}</span>
                {action.shortcut && <kbd className="chip-action-shortcut">{action.shortcut}</kbd>}
              </li>
            </>
          )}
        </ul>
      )}
    </div>
  );
}

/** The list's options in order. An unavailable one is still reached, so its status can be read. */
function optionsIn(list: HTMLUListElement | null): HTMLElement[] {
  return list ? [...list.querySelectorAll<HTMLElement>("[role=option]")] : [];
}
