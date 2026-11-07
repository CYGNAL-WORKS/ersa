import { ChevronDown, type LucideIcon, Search, ShieldAlert } from "lucide-react";
import { type CSSProperties, type KeyboardEvent, useEffect, useId, useRef, useState } from "react";
import { ContextRing } from "./ContextRing";
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
  /** The keys that do the same from anywhere on the screen, shown at the trailing edge while the action can be taken. */
  shortcut?: string;
  /** Absent while the action is shown but cannot be taken. */
  onChoose?: () => void;
}

/**
 * A second group of choices below a hairline, under a heading, narrowed by a search box: the workspace list's base
 * branch (docs/design/COMPOSER.md, The workspace list). Opening the list puts keyboard focus in its search box.
 */
export interface ChipSection {
  heading: string;
  /** The search box's placeholder, and its accessible name. */
  placeholder: string;
  /** The options matching what is typed, in order. */
  narrow: (query: string) => ChipOption[];
  /** The chosen option's id. */
  value?: string;
  onChoose: (id: string) => void;
  /** Said in place of the options when none match; also what is said while there are none to list at all. */
  empty: string;
  /**
   * The options are not there to search: being read, or not readable. The search box is unavailable, and opening
   * the list focuses the chosen option. When they arrive while the list is open, it grows to hold them.
   */
  unavailable?: boolean;
  /** `empty` says why there are none: it leads with the danger glyph. */
  problem?: boolean;
}

interface ChipProps {
  /** What the chip sets, for the accessible name: "Workspace". */
  name: string;
  /** A where chip's icon, leading it (docs/design/COMPOSER.md, The session controls): a folder for the project. */
  icon?: LucideIcon;
  /** A muted word before the value, where the value alone would not say what it is: "Effort". */
  prefix?: string;
  /** The chosen option's id; absent when nothing can be chosen. */
  value?: string;
  /** What the chip reads in place of the chosen option's label: "New workspace from main". */
  reads?: string;
  /** Shown in place of a value when `value` is absent: "No backend". */
  placeholder?: string;
  /** The value reads as a problem: the danger glyph before it. */
  problem?: boolean;
  /** The value is still being found out: the context ring's track with a turning arc before it, as backends are detected. */
  pending?: boolean;
  /** The placeholder is an invitation to act, such as Choose a folder, so it reads ink rather than ink-muted. */
  invites?: boolean;
  /** Shows a value that cannot change, as under a running session: the same pill, without its chevron, that does not open. */
  readOnly?: boolean;
  options: ChipOption[];
  onChoose: (id: string) => void;
  /** Ends the list. */
  action?: ChipAction;
  /** Follows the options, below a hairline. */
  section?: ChipSection;
  /** The list is `size-menu` wide, for options with a second line. */
  wide?: boolean;
  disabled?: boolean;
  /** For the gallery: the choices shown open, and a pointer or keyboard state shown without one. */
  initialOpen?: boolean;
  force?: "hover" | "focus" | "pressed";
  /** For the gallery: one option of an open list shown hovered, pressed or with keyboard focus. */
  forceOption?: { id: string; state: "hover" | "pressed" | "focus" };
  /** For the gallery: what an open list's search box holds. */
  initialQuery?: string;
}

/**
 * One of the session controls: a pill naming a value, which opens its choices beneath it. The keyboard opens it
 * with Enter, Space or the arrows, moves with the arrows, chooses with Enter or Space, and Escape closes it with
 * focus back on the chip.
 */
export function Chip({ name, icon, prefix, value, reads, placeholder, problem, pending, readOnly, options, onChoose, action, section, wide, disabled, initialOpen = false, force, forceOption, invites, initialQuery = "" }: ChipProps) {
  const [open, setOpen] = useState(initialOpen);
  const chip = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const listId = useId();
  // The list is anchored to its chip by this name: beneath it where the window has room, else above (Chip.css).
  const anchor = `--chip-${listId.replace(/[^a-zA-Z0-9]/g, "")}`;
  const chosen = options.find((o) => o.id === value);
  const shown = reads ?? chosen?.label ?? placeholder ?? "";
  const [query, setQuery] = useState(initialQuery);
  const narrowed = section?.narrow(query) ?? [];
  const search = useRef<HTMLInputElement>(null);

  // Focus the chosen option as the list opens from the keyboard or a click, so the arrows start from it.
  // The gallery's open chip does not take focus.
  const focusOnOpen = useRef(false);
  useEffect(() => {
    if (!open || !focusOnOpen.current) return;
    focusOnOpen.current = false;
    // A list with a search box opens with focus in it, so typing narrows at once.
    if (search.current && !search.current.disabled) return search.current.focus({ preventScroll: true });
    const items = optionsIn(list.current);
    (items.find((el) => el.dataset.id === value) ?? items[0])?.focus({ preventScroll: true });
  }, [open, value]);

  // Each opening starts with the search box empty.
  useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

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

  // Whether the list opened before its section's options arrived, so it grows to hold them when they do.
  const [openedWaiting, setOpenedWaiting] = useState(initialOpen && section?.unavailable === true);
  const show = () => {
    focusOnOpen.current = true;
    setOpenedWaiting(section?.unavailable === true);
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
  const chooseIn = (option: ChipOption) => {
    if (!section || option.unavailable || disabled) return;
    if (option.id !== section.value) section.onChoose(option.id);
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
    // In the search box the keys that edit text are its own; Enter chooses the first match.
    const typing = event.target === search.current;
    if (typing && (event.key === " " || event.key === "Home" || event.key === "End")) return;
    if (typing && event.key === "Enter") {
      event.preventDefault();
      const first = narrowed[0];
      if (first) chooseIn(first);
      return;
    }
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
        if (items[at]?.dataset.section !== undefined) {
          const inSection = narrowed.find((o) => o.id === items[at]?.dataset.id);
          if (inSection) chooseIn(inSection);
          return;
        }
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
        data-read-only={readOnly || undefined}
        // Unavailable, it reads ink-faint like any chip.
        data-invites={(invites && chosen === undefined && !disabled) || undefined}
        style={{ anchorName: anchor } as CSSProperties}
        disabled={disabled || readOnly}
        onClick={() => (open ? setOpen(false) : show())}
        onKeyDown={onChipKey}
      >
        {problem && <span className="glyph" data-tone="danger" aria-hidden="true" />}
        {pending && <ContextRing turning />}
        {icon && (
          <span className="chip-icon">
            <Icon icon={icon} size="icon-xs" />
          </span>
        )}
        {prefix && <span className="chip-prefix">{prefix}</span>}
        <span className="chip-value">{shown}</span>
        {/* A chip that opens a list ends with a chevron; one that cannot change, or is waiting, does not. */}
        {!readOnly && !pending && (
          <span className="chip-chevron">
            <Icon icon={ChevronDown} size="icon-xs" />
          </span>
        )}
      </button>
      {open && (
        <ul ref={list} id={listId} className="chip-list" data-wide={wide || undefined} style={{ positionAnchor: anchor } as CSSProperties} role="listbox" aria-label={name} onKeyDown={onListKey}>
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
          {section && (
            <>
              <li className="chip-list-rule" role="presentation" />
              <li className="chip-section" role="presentation">
                <span className="chip-section-heading" id={`${listId}-section`}>
                  {section.heading}
                </span>
                <span className="chip-search">
                  <span className="chip-search-icon">
                    <Icon icon={Search} size="icon-sm" />
                  </span>
                  <input
                    ref={search}
                    className="chip-search-input"
                    type="search"
                    // Reached by the arrows only while it can be used.
                    data-item={section.unavailable ? undefined : ""}
                    disabled={section.unavailable}
                    aria-label={section.placeholder}
                    placeholder={section.placeholder}
                    value={query}
                    onChange={(event) => setQuery(event.target.value)}
                  />
                </span>
              </li>
              <li className="chip-section-options" role="presentation">
                <ul role="group" aria-labelledby={`${listId}-section`} data-grow={(openedWaiting && !section.unavailable) || undefined}>
                  {narrowed.map((option) => (
                    <li
                      key={option.id}
                      role="option"
                      tabIndex={-1}
                      className="chip-option"
                      data-id={option.id}
                      data-section=""
                      aria-selected={option.id === section.value}
                      aria-disabled={option.unavailable || undefined}
                      onClick={() => chooseIn(option)}
                    >
                      <span className="chip-option-mark" aria-hidden="true" />
                      <span className="chip-option-label">{option.label}</span>
                      {option.status && <span className="chip-option-status">{option.status}</span>}
                    </li>
                  ))}
                </ul>
                {narrowed.length === 0 && (
                  <p className="chip-section-note">
                    {section.problem && <span className="glyph" data-tone="danger" aria-hidden="true" />}
                    {section.empty}
                  </p>
                )}
              </li>
            </>
          )}
          {action && (
            <>
              {/* The hairline parts the action from options above it; alone in the list, it needs none. */}
              {options.length > 0 && <li className="chip-list-rule" role="presentation" />}
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
                {action.shortcut && action.onChoose && <kbd className="chip-action-shortcut">{action.shortcut}</kbd>}
              </li>
            </>
          )}
        </ul>
      )}
    </div>
  );
}

/**
 * The list's options in order, with a section's search box among them, so the arrows reach it. An unavailable option
 * is still reached, so its status can be read.
 */
function optionsIn(list: HTMLUListElement | null): HTMLElement[] {
  return list ? [...list.querySelectorAll<HTMLElement>("[role=option], [data-item]")] : [];
}
