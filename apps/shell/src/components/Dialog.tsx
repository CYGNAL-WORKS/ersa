import { type KeyboardEvent, type ReactNode, type RefObject, useEffect, useId, useLayoutEffect, useRef } from "react";
import { HeightEase } from "../theme/motion";
import "./Dialog.css";

interface DialogProps {
  /** The question, as the dialog's heading and accessible name. */
  title: ReactNode;
  /** What it says beneath the title, all that can change its height; `detail` is its description. */
  children: ReactNode;
  /** The words that describe the dialog to assistive technology, first in the body. */
  detail: ReactNode;
  /** The buttons at its foot. */
  actions: ReactNode;
  /** Work under way: assistive technology hears the dialog is busy. */
  busy?: boolean;
  /** Escape: the caller ignores it while work it can't stop is under way. */
  onCancel: () => void;
  /** Where focus goes as the dialog opens and whenever `focusKey` changes; the dialog itself when undefined. */
  focusTo?: RefObject<HTMLElement | null>;
  /** Changes when what the dialog says changes: its height eases to the new one, and focus goes to `focusTo`. */
  focusKey: unknown;
  /** For the gallery: drawn in place, taking no focus, rather than over the window. */
  inline?: boolean;
}

/**
 * A question asked over the window before something that can't be undone (docs/design/RAIL.md, The delete question;
 * docs/design/SETTINGS.md, The clean-up question): centered and above every layer, the window behind it not dimmed.
 * Focus stays within it, Tab wrapping from the last button to the first; a press elsewhere takes no focus; Escape
 * asks to cancel. Its height eases as what it says changes, so nothing pops in.
 */
export function Dialog({ title, children, detail, actions, busy = false, onCancel, focusTo, focusKey, inline = false }: DialogProps) {
  const id = useId();
  const root = useRef<HTMLDivElement>(null);

  // Each phase places focus as it begins, so it never falls out of the dialog with a button that became unavailable.
  useEffect(() => {
    if (inline) return;
    (focusTo?.current ?? root.current)?.focus({ preventScroll: true });
  }, [inline, focusKey, focusTo]);

  // Focus stays within the dialog: a press anywhere else in the window, all of it inert while the question is open,
  // takes no focus, so Escape and Tab still reach the dialog.
  useEffect(() => {
    if (inline) return;
    const away = (event: MouseEvent) => {
      if (event.target instanceof Node && root.current?.contains(event.target)) return;
      event.preventDefault();
    };
    document.addEventListener("mousedown", away, true);
    return () => document.removeEventListener("mousedown", away, true);
  }, [inline]);

  // What arrives late, counts or a failure's reason, changes the dialog's height. It eases to the new height over
  // motion-settle and stays centered (DESIGN.md); its body takes up the difference, so the title stays at its top and
  // the buttons at its foot, growing or shrinking.
  const ease = useRef(new HeightEase()).current;
  useLayoutEffect(() => {
    if (!inline && root.current) ease.change(root.current);
  }, [ease, inline, focusKey]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.stopPropagation();
      onCancel();
    } else if (event.key === "Tab") {
      // Focus stays within the dialog: Tab from the last button wraps to the first, and Shift+Tab the other way.
      const buttons = [...(root.current?.querySelectorAll<HTMLElement>("button:not(:disabled)") ?? [])];
      const first = buttons[0];
      const last = buttons[buttons.length - 1];
      if (!first || !last) {
        event.preventDefault();
        return;
      }
      if (event.shiftKey && (document.activeElement === first || !root.current?.contains(document.activeElement))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (document.activeElement === last || !root.current?.contains(document.activeElement))) {
        event.preventDefault();
        first.focus();
      }
    }
  };

  return (
    <div
      ref={root}
      className="dialog"
      data-inline={inline || undefined}
      role="dialog"
      aria-modal={!inline || undefined}
      aria-labelledby={`${id}-title`}
      aria-describedby={`${id}-detail`}
      aria-busy={busy || undefined}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      <h2 className="dialog-title" id={`${id}-title`}>
        {title}
      </h2>
      <div className="dialog-body">
        <div className="dialog-detail" id={`${id}-detail`}>
          {detail}
        </div>
        {children}
      </div>
      <div className="dialog-actions">{actions}</div>
    </div>
  );
}

/** What would be lost, led by a caution glyph, in ink; nothing when nothing is. */
export function DialogLoss({ children }: { children: ReactNode }) {
  return (
    <p className="dialog-loss">
      <span className="glyph" data-tone="caution" aria-hidden="true" />
      <span>{children}</span>
    </p>
  );
}

/** Why the work failed: the danger glyph's lead and the reason beneath it, keeping its own lines. */
export function DialogFailure({ lead, reason }: { lead: string; reason: string }) {
  return (
    <div className="dialog-failed" role="alert">
      <p className="problem">{lead}</p>
      <p className="dialog-reason">{reason}</p>
    </div>
  );
}
