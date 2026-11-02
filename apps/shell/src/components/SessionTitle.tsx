import { type ReactNode, useEffect, useRef, useState } from "react";
import type { Sending } from "../session/follow";
import { Button } from "./Button";

/** Says how to rename, to a pointer's tooltip and to a screen reader. */
const RENAME_HINT = "Double-click, or press Enter or F2, to rename";
import "./SessionTitle.css";

interface SessionTitleProps {
  title: string;
  /** The title is a placeholder: the backend has given none yet, so editing starts from nothing. */
  untitled?: boolean;
  /** Renames the session in its backend; resolves true once Core has. Absent: the session cannot be renamed here. */
  onRename?: (title: string) => Promise<boolean>;
  renaming?: Sending;
  /** Editing starts or ends: a refusal from an earlier attempt is no longer the user's concern. */
  onReset?: () => void;
  /** Renaming is there but cannot be used now, such as while Core is away. */
  disabled?: boolean;
  /** What follows the title in the header, such as the session's state. */
  children?: ReactNode;
  /** For the gallery: the title already being edited, with this text, and a pointer or keyboard state shown without one. */
  initialDraft?: string;
  force?: "hover" | "focus";
}

/**
 * The title to send for `draft`, or undefined when there is nothing to send: a blank title, which no backend
 * keeps, or the title the session already has (`current`, empty for one with none). Spacing is Core's to settle,
 * by one rule, and the title Core answers with is the one shown.
 */
export function titleToSave(draft: string, current: string): string | undefined {
  const title = draft.trim();
  return title === "" || title === current ? undefined : title;
}

/** What saving `draft` sends for a session showing `title`: an untitled session's placeholder is not its title. */
export function renameTo(draft: string, title: string, untitled: boolean | undefined): string | undefined {
  return titleToSave(draft, untitled ? "" : title);
}

/** The session's title in its header, and renaming it there: the backend keeps the title, so its own tools show it too. */
export function SessionTitle({ title, untitled, onRename, renaming, onReset, disabled, children, initialDraft, force }: SessionTitleProps) {
  // The text being edited; undefined while the title is only shown.
  const [draft, setDraft] = useState<string | undefined>(initialDraft);
  const busy = renaming?.state === "sending";
  const heading = useRef<HTMLHeadingElement>(null);
  const field = useRef<HTMLInputElement>(null);
  // Focus goes back to the title once editing ends, where the user started it.
  const returnFocus = useRef(false);
  // The old title is selected when the field first takes focus, to type over; not again, or a refusal's return
  // to the field would select the draft being corrected.
  const selected = useRef(false);

  useEffect(() => {
    if (draft === undefined && returnFocus.current) {
      returnFocus.current = false;
      heading.current?.focus();
    }
  }, [draft]);

  // A refusal puts the user back in the field to try again: a Save clicked with the pointer was disabled while
  // saving, which dropped focus.
  const refused = renaming?.state === "failed";
  useEffect(() => {
    if (refused && initialDraft === undefined) field.current?.focus();
  }, [refused, initialDraft]);

  const close = () => {
    returnFocus.current = true;
    setDraft(undefined);
    onReset?.();
  };

  const save = async () => {
    if (draft === undefined || busy) return;
    const next = renameTo(draft, title, untitled);
    if (next === undefined) {
      // A blank title is not saved; an unchanged one needs no saving.
      if (draft.trim() !== "") close();
      return;
    }
    if (await onRename?.(next)) close();
  };

  // While a rename is still on its way, a second waits for its answer.
  const editable = !!onRename && !disabled && !busy;
  const edit = () => {
    if (!editable) return;
    onReset?.();
    selected.current = false;
    setDraft(untitled ? "" : title);
  };

  if (draft === undefined || !onRename) {
    // The title itself is the control: no button beside it. A pointer double-clicks it; the keyboard focuses it and
    // presses Enter or F2, as in a file list.
    return (
      <>
        <h1
          ref={heading}
          className="session-heading"
          data-editable={editable || undefined}
          data-force={editable ? force : undefined}
          tabIndex={editable ? 0 : undefined}
          title={editable ? RENAME_HINT : undefined}
          aria-description={editable ? RENAME_HINT : undefined}
          onDoubleClick={edit}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === "F2") {
              event.preventDefault();
              edit();
            }
          }}
        >
          {title}
        </h1>
        <span className="session-heading-state">{children}</span>
      </>
    );
  }

  return (
    <>
      <form
        className="session-heading-form"
        aria-label="Rename the session"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <input
          ref={field}
          className="session-heading-input"
          aria-label="Session title"
          value={draft}
          readOnly={busy}
          data-force={force}
          // Opened by the user: they type the new title at once, over the old one.
          autoFocus={initialDraft === undefined}
          onFocus={(event) => {
            if (initialDraft === undefined && !selected.current) {
              selected.current = true;
              event.target.select();
            }
          }}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape" && !busy) {
              event.preventDefault();
              close();
            }
          }}
        />
        <Button type="submit" disabled={busy || disabled || draft.trim() === ""}>
          {/* Both labels share one cell, so the button keeps its width and nothing beside it moves. */}
          <span className="session-heading-label">
            <span data-shown={!busy || undefined}>Save</span>
            <span data-shown={busy || undefined}>Saving…</span>
          </span>
        </Button>
        <Button disabled={busy} onClick={close}>
          Cancel
        </Button>
        <span className="session-heading-state">{children}</span>
        {renaming?.state === "failed" && (
          <p className="problem session-heading-problem" role="alert">
            {renaming.message}
          </p>
        )}
      </form>
    </>
  );
}
