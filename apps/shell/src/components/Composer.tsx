import { type KeyboardEvent, type Ref, useState } from "react";
import type { Sending } from "../session/follow";
import { Button } from "./Button";
import "./Composer.css";

interface ComposerProps {
  /** Why the user cannot write now, shown in place of the hint; absent when they can. */
  unavailable?: string;
  sending?: Sending;
  /** Sends the text; resolves true once Core accepted it, and the composer clears. */
  onSend: (text: string) => Promise<boolean>;
  /** The user changed the text. */
  onWrite?: () => void;
  /** For the gallery: text already typed, and a pointer or keyboard state shown without one. */
  initialText?: string;
  force?: "hover" | "focus";
  /** Whether the composer takes focus when it appears: when a session opens (FLOW-NEW-SESSION). Off in the gallery. */
  autoFocus?: boolean;
  /** The words, for a screen that returns focus to them, as after the folder dialog or the trust question. */
  inputRef?: Ref<HTMLTextAreaElement>;
}

/** Where the user writes to the agent. Enter sends; Shift+Enter starts a new line. */
export function Composer({ unavailable, sending, onSend, onWrite, initialText = "", force, autoFocus = true, inputRef }: ComposerProps) {
  const [text, setText] = useState(initialText);
  const busy = sending?.state === "sending";
  const canSend = !unavailable && !busy && text.trim() !== "";

  const send = async () => {
    if (!canSend) return;
    const sent = text;
    // Clear only what was sent: anything typed meanwhile is the next message.
    if (await onSend(sent)) setText((now) => (now === sent ? "" : now));
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      void send();
    }
  };

  return (
    <form
      className="composer"
      aria-label="Composer"
      onSubmit={(event) => {
        event.preventDefault();
        void send();
      }}
    >
      <textarea
        ref={inputRef}
        className="composer-input"
        aria-label="Message"
        placeholder="Write to the agent"
        rows={3}
        autoFocus={autoFocus}
        data-force={force}
        value={text}
        disabled={!!unavailable}
        onChange={(event) => {
          setText(event.target.value);
          onWrite?.();
        }}
        onKeyDown={onKeyDown}
      />
      <div className="composer-foot">
        {sending?.state === "failed" ? (
          <p className="problem" role="alert">
            {sending.message}
          </p>
        ) : (
          <p className="composer-hint">{unavailable ?? (busy ? "Sending…" : "Enter to send, Shift+Enter for a new line")}</p>
        )}
        <Button type="submit" disabled={!canSend}>
          Send
        </Button>
      </div>
    </form>
  );
}
