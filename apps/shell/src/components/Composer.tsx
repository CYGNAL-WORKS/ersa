import { ArrowUp, Plus } from "lucide-react";
import { type KeyboardEvent, type Ref, useEffect, useImperativeHandle, useRef, useState } from "react";
import { t } from "../catalog";
import type { Sending } from "../session/follow";
import { Icon, IconButton } from "./Icon";
import "./Composer.css";

/**
 * Why the box is unavailable (docs/design/COMPOSER.md, The status line): Ersa can't reach the sessions and is trying
 * again, or has stopped trying (FLOW-SEND-MESSAGE-R06); the host's backends are being detected
 * (FLOW-NEW-SESSION-R23); or the host has no backend to run, none installed or, by its label, one that failed
 * detection (FLOW-NEW-SESSION-R07).
 */
export type Unavailable = "reconnecting" | "unreachable" | "detecting" | "no-backend" | { failedBackend: string };

interface ComposerProps {
  /** Why the user cannot send now; absent when they can. */
  unavailable?: Unavailable;
  /** The last send: on its way, or refused with the reason, which the status line shows (FLOW-SEND-MESSAGE-R05). */
  sending?: Sending;
  /** Sends the text; resolves true once Core accepted it, and the composer clears. */
  onSend: (text: string) => Promise<boolean>;
  /** The user changed the text. */
  onWrite?: () => void;
  /** Three lines tall, as on the new-session screen; one line otherwise, as under a session. Either grows with the words. */
  tall?: boolean;
  /** What a screen opens above the box in the status line's place, such as the trust question: the line says nothing meanwhile. */
  statusTaken?: boolean;
  /** A permission request is open and holds the focal glow, so focus shows on the box's edge alone. */
  quietFocus?: boolean;
  /** For the gallery: text already typed, and a pointer or keyboard state shown without one. */
  initialText?: string;
  force?: "hover" | "focus";
  /** Whether the composer takes focus when it appears: when a session opens (FLOW-NEW-SESSION). Off in the gallery. */
  autoFocus?: boolean;
  /** The words, for a screen that returns focus to them, as after the folder dialog or the trust question. */
  inputRef?: Ref<HTMLTextAreaElement>;
}

/**
 * Where the user writes to the agent (docs/design/COMPOSER.md): the status line above the box, and the box holding
 * attach, the words and send. Enter sends; Shift+Enter starts a new line (FLOW-SEND-MESSAGE-R01).
 */
export function Composer({ unavailable, sending, onSend, onWrite, tall, statusTaken, quietFocus, initialText = "", force, autoFocus = true, inputRef }: ComposerProps) {
  const [text, setText] = useState(initialText);
  const words = useRef<HTMLTextAreaElement>(null);
  useImperativeHandle(inputRef, () => words.current as HTMLTextAreaElement);
  const busy = sending?.state === "sending";
  const canSend = !unavailable && !busy && text.trim() !== "";

  // When the box becomes unavailable while the user is in it, focus goes back to the words once it is available
  // again. Read as the box turns unavailable, before the words are disabled and lose focus.
  const wasUnavailable = useRef(unavailable !== undefined);
  const refocus = useRef(false);
  if (unavailable !== undefined && !wasUnavailable.current) {
    const at = typeof document === "undefined" ? null : document.activeElement;
    refocus.current = at !== null && at === words.current;
  }
  wasUnavailable.current = unavailable !== undefined;
  useEffect(() => {
    if (unavailable === undefined && refocus.current) {
      refocus.current = false;
      // Only if focus went nowhere meanwhile: a user who moved on, to the rail say, keeps their place.
      const at = document.activeElement;
      if (at === null || at === document.body) words.current?.focus({ preventScroll: true });
    }
  }, [unavailable]);

  const send = async () => {
    if (!canSend) return;
    const sent = text;
    // Clear only what was sent: anything typed meanwhile is the next message (FLOW-SEND-MESSAGE-R02).
    if (await onSend(sent)) setText((now) => (now === sent ? "" : now));
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      void send();
    }
  };

  return (
    <div className="composer">
      {/* Takes no height and grows upward, so the box and the session controls stay where they are. */}
      <div className="composer-status">
        {!statusTaken && <StatusLine unavailable={unavailable} sending={sending} />}
      </div>
      <form
        className="composer-box"
        aria-label={t("composer.name")}
        data-unavailable={unavailable !== undefined || undefined}
        data-quiet-focus={quietFocus || undefined}
        data-force={force}
        onSubmit={(event) => {
          event.preventDefault();
          void send();
        }}
      >
        {/* Its menu comes with attachments (FLOW-SEND-MESSAGE-R07); until then it opens nothing. */}
        <IconButton icon={Plus} label={t("composer.attachButton")} className="composer-attach" disabled={unavailable !== undefined} />
        <textarea
          ref={words}
          className="composer-words"
          aria-label={t("composer.messageLabel")}
          placeholder={t("composer.writePlaceholder")}
          rows={tall ? 3 : 1}
          data-tall={tall || undefined}
          autoFocus={autoFocus}
          value={text}
          disabled={unavailable !== undefined}
          onChange={(event) => {
            setText(event.target.value);
            onWrite?.();
          }}
          onKeyDown={onKeyDown}
        />
        <button type="submit" className="composer-send" aria-label={t("composer.sendButton")} disabled={!canSend}>
          <Icon icon={ArrowUp} size="icon" />
        </button>
      </form>
    </div>
  );
}

/**
 * One message at a time, first that Ersa can't reach the sessions, then a refused message, then detecting backends
 * (docs/design/COMPOSER.md, The status line); nothing, and no height, while there is nothing to say.
 */
function StatusLine({ unavailable, sending }: { unavailable?: Unavailable; sending?: Sending }) {
  if (unavailable === "reconnecting" || unavailable === "unreachable") {
    // The rail's foot says the same and is the one that announces it.
    return <p className="composer-status-line">{t(unavailable === "reconnecting" ? "rail.foot.reconnectingError" : "rail.foot.unreachableError")}</p>;
  }
  if (sending?.state === "failed") {
    return (
      <p className="composer-status-line" role="alert">
        <span className="glyph" data-tone="danger" aria-hidden="true" />
        {sending.message}
      </p>
    );
  }
  if (unavailable === "detecting") return <p className="composer-status-line">{t("composer.detectingProgress")}</p>;
  if (unavailable === "no-backend") return <p className="composer-status-line">{t("composer.noBackendError")}</p>;
  if (typeof unavailable === "object") {
    return <p className="composer-status-line">{t("composer.backendFailedError", { label: unavailable.failedBackend })}</p>;
  }
  return null;
}
