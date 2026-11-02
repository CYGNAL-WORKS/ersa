import { type ReactNode, useEffect, useRef, useState } from "react";
import type { Sending } from "../session/follow";
import { Button } from "./Button";
import "./SessionTitle.css";

interface SessionTitleProps {
  title: string;
  /** Renames the session in its backend; resolves true once Core has. Absent: the session cannot be renamed here. */
  onRename?: (title: string) => Promise<boolean>;
  renaming?: Sending;
  /** Editing has ended, saved or not: a refusal from before is no longer the user's concern. */
  onDone?: () => void;
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
 * keeps, or the title the session already has.
 */
export function titleToSave(draft: string, current: string): string | undefined {
  const title = draft.trim();
  return title === "" || title === current ? undefined : title;
}

/** The session's title in its header, and renaming it there: the backend keeps the title, so its own tools show it too. */
export function SessionTitle({ title, onRename, renaming, onDone, disabled, children, initialDraft, force }: SessionTitleProps) {
  // The text being edited; undefined while the title is only shown.
  const [draft, setDraft] = useState<string | undefined>(initialDraft);
  const busy = renaming?.state === "sending";
  const rename = useRef<HTMLButtonElement>(null);
  const field = useRef<HTMLInputElement>(null);
  // Focus goes back to Rename once editing ends, where the user started it.
  const returnFocus = useRef(false);

  useEffect(() => {
    if (draft === undefined && returnFocus.current) {
      returnFocus.current = false;
      rename.current?.focus();
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
    onDone?.();
  };

  const save = async () => {
    if (draft === undefined || busy) return;
    const next = titleToSave(draft, title);
    if (next === undefined) {
      // A blank title is not saved; an unchanged one needs no saving.
      if (draft.trim() !== "") close();
      return;
    }
    if (await onRename?.(next)) close();
  };

  if (draft === undefined || !onRename) {
    return (
      <>
        <h1 className="session-heading">{title}</h1>
        {children}
        {onRename && (
          <Button
            ref={rename}
            className="session-heading-rename"
            force={force}
            disabled={disabled}
            onClick={() => setDraft(title)}
          >
            Rename
          </Button>
        )}
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
          // As long as Core takes: as much as the backend's own session list shows.
          maxLength={200}
          value={draft}
          readOnly={busy}
          data-force={force}
          // Opened by the user: they type the new title at once, over the old one.
          autoFocus={initialDraft === undefined}
          onFocus={(event) => {
            if (initialDraft === undefined) event.target.select();
          }}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape" && !busy) {
              event.preventDefault();
              close();
            }
          }}
        />
        {/* The state stays beside the title, and the actions at the far edge, where Rename was. */}
        {children}
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
        {renaming?.state === "failed" && (
          <p className="problem session-heading-problem" role="alert">
            {renaming.message}
          </p>
        )}
      </form>
    </>
  );
}
