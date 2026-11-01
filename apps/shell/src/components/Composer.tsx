import { type KeyboardEvent, useState } from "react";
import type { Sending } from "../session/follow";
import { Button } from "./Button";
import "./Composer.css";

interface ComposerProps {
  /** Why the user cannot write now, shown in place of the hint; absent when they can. */
  unavailable?: string;
  sending?: Sending;
  /** Sends the text; resolves true once Core accepted it, and the composer clears. */
  onSend: (text: string) => Promise<boolean>;
  /** For the gallery: text already typed. */
  initialText?: string;
}

/** Where the user writes to the agent. Enter sends; Shift+Enter starts a new line. */
export function Composer({ unavailable, sending, onSend, initialText = "" }: ComposerProps) {
  const [text, setText] = useState(initialText);
  const busy = sending?.state === "sending";
  const canSend = !unavailable && !busy && text.trim() !== "";

  const send = async () => {
    if (!canSend) return;
    if (await onSend(text)) setText("");
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
        className="composer-input"
        aria-label="Message"
        placeholder="Write to the agent"
        rows={3}
        value={text}
        disabled={!!unavailable}
        onChange={(event) => setText(event.target.value)}
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
