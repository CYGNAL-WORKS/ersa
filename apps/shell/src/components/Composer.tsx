import { ArrowUp, File, Folder, Image, Square, X } from "lucide-react";
import { type KeyboardEvent, type ReactNode, type Ref, useEffect, useImperativeHandle, useRef, useState } from "react";
import { t } from "../catalog";
import { type Attached, addAll, outside, same } from "../session/attachments";
import type { Sending } from "../session/follow";
import { type AttachWhat, AttachMenu } from "./AttachMenu";
import { ContextRing } from "./ContextRing";
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
  /** Sends the text and attachments; resolves true once Core accepted them, and the composer clears. */
  onSend: (text: string, attachments: readonly Attached[]) => Promise<boolean>;
  /**
   * Opens the system's dialog for files, images or a folder and reads each path picked (FLOW-SEND-MESSAGE-R07, R13);
   * absent where nothing can be attached, and attach is then unavailable.
   */
  onAttach?: (what: AttachWhat) => Promise<Attached[]>;
  /** The folder of the session's workspace; absent while it is not made yet, so every attachment is from outside it (R14). */
  workspace?: string;
  /** Attachments already added: a message given back to send again, or the gallery's. */
  initialAttachments?: Attached[];
  /** For the gallery: the attach menu shown open, one of its choices in a pointer or keyboard state. */
  attachMenuOpen?: boolean;
  forceAttachChoice?: { what: AttachWhat; state: "hover" | "pressed" | "focus" };
  /** The user changed the text. */
  onWrite?: () => void;
  /** Three lines tall, as on the new-session screen; one line otherwise, as under a session. Either grows with the words. */
  tall?: boolean;
  /** What a screen opens above the box in the status line's place, such as the trust question: the line says nothing meanwhile. */
  statusTaken?: boolean;
  /** The session controls, beneath the box in the same column, space-xs below it (docs/design/COMPOSER.md). */
  controls?: ReactNode;
  /** The agent is working on a turn: Enter does not send, and what is written stays for when it ends (FLOW-SEND-MESSAGE-R04). */
  working?: boolean;
  /** Stop: offered in send's place from the moment the user sends until the turn ends (FLOW-SEND-MESSAGE-R03). */
  onStop?: () => void;
  /** For the gallery: send or stop shown under the pointer, pressed, or with keyboard focus. */
  forceSend?: "hover" | "pressed" | "focus";
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
export function Composer({ unavailable, sending, onSend, onWrite, onAttach, workspace, initialAttachments, attachMenuOpen, forceAttachChoice, tall, statusTaken, controls, forceSend, working, onStop, quietFocus, initialText = "", force, autoFocus = true, inputRef }: ComposerProps) {
  const [text, setText] = useState(initialText);
  const [attachments, setAttachments] = useState<Attached[]>(initialAttachments ?? []);
  const words = useRef<HTMLTextAreaElement>(null);
  useImperativeHandle(inputRef, () => words.current as HTMLTextAreaElement);
  const busy = sending?.state === "sending";
  // An attachment Ersa can't send holds the message until it is removed (FLOW-SEND-MESSAGE-R10).
  const held = attachments.some((a) => a.problem !== undefined);
  const canSend = !unavailable && !busy && !working && !held && (text.trim() !== "" || attachments.length > 0);

  // The attachments last sent, so a refusal naming one by its place marks that one (R10), even if others went since.
  const sent = useRef<readonly Attached[]>([]);
  const refused = sending?.state === "failed" ? sending : undefined;
  useEffect(() => {
    const at = refused?.index;
    const which = at === undefined ? undefined : sent.current[at];
    if (!refused || !which) return;
    setAttachments((now) => now.map((a) => (same(a.path, which.path) ? { ...a, problem: refused.message } : a)));
  }, [refused]);

  // Removing one moves keyboard focus to the next one's remove, else to the words (docs/design/COMPOSER.md).
  const list = useRef<HTMLUListElement>(null);
  const focusAfter = useRef<number | undefined>(undefined);
  useEffect(() => {
    const at = focusAfter.current;
    if (at === undefined) return;
    focusAfter.current = undefined;
    const removes = list.current?.querySelectorAll<HTMLButtonElement>(".composer-attachment-remove");
    (removes?.[at] ?? words.current)?.focus({ preventScroll: true });
  }, [attachments]);
  const remove = (at: number) => {
    focusAfter.current = at;
    setAttachments((now) => now.filter((_, i) => i !== at));
  };

  const attach = async (what: AttachWhat) => {
    if (!onAttach) return;
    const more = await onAttach(what);
    // The same file or folder is attached once (R12).
    if (more.length > 0) setAttachments((now) => addAll(now, more));
  };
  // From the moment of sending until the turn ends, the button is stop.
  const stopping = (busy || !!working) && onStop !== undefined && !unavailable;
  // The icon fades in only when send and stop trade places, not as the composer appears.
  const [traded, setTraded] = useState(false);
  const wasStopping = useRef(stopping);
  if (wasStopping.current !== stopping) {
    wasStopping.current = stopping;
    if (!traded) setTraded(true);
  }

  // When the box becomes unavailable while the user is in it, focus goes back where it was, the words or the
  // send-or-stop button, once it is available again. Read as the box turns unavailable, before they are disabled.
  const button = useRef<HTMLButtonElement>(null);
  const wasUnavailable = useRef(unavailable !== undefined);
  const refocus = useRef<HTMLElement | null>(null);
  if (unavailable !== undefined && !wasUnavailable.current) {
    const at = typeof document === "undefined" ? null : document.activeElement;
    refocus.current = at === words.current || at === button.current ? (at as HTMLElement) : null;
  }
  wasUnavailable.current = unavailable !== undefined;
  useEffect(() => {
    if (unavailable === undefined && refocus.current) {
      const back = refocus.current;
      refocus.current = null;
      // Only if focus went nowhere meanwhile: a user who moved on, to the rail say, keeps their place.
      const at = document.activeElement;
      if (at === null || at === document.body) back.focus({ preventScroll: true });
    }
  }, [unavailable]);

  const send = async () => {
    if (!canSend) return;
    const written = text;
    const going = attachments;
    sent.current = going;
    // Clear only what was sent: anything typed or attached meanwhile is the next message (FLOW-SEND-MESSAGE-R02).
    if (await onSend(written, going)) {
      setText((now) => (now === written ? "" : now));
      setAttachments((now) => now.filter((a) => !going.includes(a)));
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      // While a message is on its way or the agent works, Enter does nothing and the words stay.
      event.preventDefault();
      void send();
    }
  };

  return (
    <div className="composer">
      {/* Takes no height and grows upward, so the box and the session controls stay where they are. */}
      <div className="composer-status">
        {!statusTaken && <StatusLine unavailable={unavailable} sending={sending} marked={held} />}
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
        {attachments.length > 0 && (
          <ul ref={list} className="composer-attachments" aria-label={t("composer.attachment.listLabel")}>
            {attachments.map((a, i) => (
              <Attachment key={a.path} attachment={a} outside={outside(a, workspace)} onRemove={() => remove(i)} />
            ))}
          </ul>
        )}
        <AttachMenu
          onChoose={onAttach && ((what) => void attach(what))}
          disabled={unavailable !== undefined}
          initialOpen={attachMenuOpen}
          forceChoice={forceAttachChoice}
        />
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
        {/* One button, send or stop, so keyboard focus stays on it as it turns from one to the other. With nothing to
            send it stays focusable, so focus on stop survives the turn ending; unavailable, it is disabled. */}
        <button
          ref={button}
          type={stopping ? "button" : "submit"}
          className="composer-send"
          data-stop={stopping || undefined}
          data-force={forceSend}
          aria-label={t(stopping ? "composer.stopButton" : "composer.sendButton")}
          aria-disabled={(!stopping && !canSend) || undefined}
          disabled={unavailable !== undefined}
          onClick={(event) => {
            if (stopping) {
              event.preventDefault();
              onStop?.();
            } else if (!canSend) {
              event.preventDefault();
            }
          }}
        >
          {/* Keyed, so the icon fades in as send and stop trade places. */}
          <span key={stopping ? "stop" : "send"} className="composer-send-icon" data-traded={traded || undefined}>
            <Icon icon={stopping ? Square : ArrowUp} size={stopping ? "icon-sm" : "icon"} />
          </span>
        </button>
      </form>
      {controls}
    </div>
  );
}

const KIND_ICON = { file: File, image: Image, folder: Folder } as const;

/**
 * One attachment (docs/design/COMPOSER.md, Attachments): its icon, name and detail, that it will be copied into the
 * workspace when it comes from outside it (FLOW-SEND-MESSAGE-R14), and remove (R09). One Ersa can't send shows the
 * danger glyph and why in place of its icon and detail (R10).
 */
function Attachment({ attachment: a, outside, onRemove }: { attachment: Attached; outside: boolean; onRemove: () => void }) {
  return (
    <li className="composer-attachment" data-problem={a.problem !== undefined || undefined}>
      {a.problem !== undefined ? (
        <span className="glyph" data-tone="danger" aria-hidden="true" />
      ) : (
        <span className="composer-attachment-icon">
          <Icon icon={KIND_ICON[a.kind]} size="icon-sm" />
        </span>
      )}
      <span className="composer-attachment-name">{a.name}</span>
      {a.problem !== undefined ? (
        <span className="composer-attachment-problem" role="alert">
          {a.problem}
        </span>
      ) : (
        <>
          <span className="composer-attachment-detail">{a.detail}</span>
          {outside && <span className="composer-attachment-detail">{t("composer.attachment.copiedNote")}</span>}
        </>
      )}
      <IconButton icon={X} size="icon-xs" label={t("composer.attachment.removeLabel", { name: a.name })} className="composer-attachment-remove" onClick={onRemove} />
    </li>
  );
}

/**
 * One message at a time, first that Ersa can't reach the sessions, then a send whose outcome Ersa can't tell, then a
 * refused message, then detecting backends, then a host with no backend, then a send on its way
 * (docs/design/COMPOSER.md, The status line); nothing, and no height, while there is nothing to say. A message refused
 * for one of its attachments says why beside that attachment instead.
 */
function StatusLine({ unavailable, sending, marked }: { unavailable?: Unavailable; sending?: Sending; marked: boolean }) {
  if (unavailable === "reconnecting" || unavailable === "unreachable") {
    // The rail's foot says the same and is the one that announces it.
    return <p className="composer-status-line">{t(unavailable === "reconnecting" ? "rail.foot.reconnectingError" : "rail.foot.unreachableError")}</p>;
  }
  if (sending?.state === "unknown") {
    // Never that it failed: it may have arrived (FLOW-SEND-MESSAGE-R15).
    return (
      <p className="composer-status-line" role="status">
        <span className="glyph" data-tone="caution" aria-hidden="true" />
        {t("composer.unknownNote")}
      </p>
    );
  }
  // A refusal naming an attachment is said beside it; when none is marked, as one already removed, it is said here.
  if (sending?.state === "failed" && (sending.index === undefined || !marked)) {
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
  const progress = sending?.state === "sending" ? sending.progress : undefined;
  if (progress?.phase === "copying") {
    return (
      <p className="composer-status-line">
        <ContextRing used={progress.share} mark />
        {t("composer.copyingProgress", { percent: Math.round(progress.share * 100) })}
      </p>
    );
  }
  if (progress?.phase === "starting") {
    return (
      <p className="composer-status-line">
        <ContextRing turning />
        {t("composer.startingProgress")}
      </p>
    );
  }
  return null;
}
