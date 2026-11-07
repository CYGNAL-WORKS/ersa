import { type ReactNode, memo, useLayoutEffect, useRef, useState } from "react";
import { t } from "../catalog";
import type { PermissionRequested, ToolInput } from "../protocol/agent-events.gen";
import type { Answer, Decision } from "../session/answers";
import type { SessionView, Sending } from "../session/follow";
import { pathName } from "../session/attachments";
import { stateText } from "../session/states";
import { type Item, type MessageAttachment, isOpen } from "../session/transcript";
import { Button } from "./Button";
import { AttachmentChip } from "./AttachmentChip";
import { PermissionAnswers } from "./PermissionAnswers";
import { SessionTitle } from "./SessionTitle";
import "./Transcript.css";

type Decide = (requestId: string, decision: Decision) => void;

interface TranscriptProps {
  title: string;
  /** The title is a placeholder; see SessionTitle. */
  untitled?: boolean;
  view: SessionView;
  /** The session's answers on their way or refused, by request id (answers.ts). */
  answers?: Readonly<Record<string, Answer>>;
  /** Kept stable across renders, so streaming text does not re-render every block. */
  onDecide: Decide;
  /** Subscribes again after subscribing failed. */
  onRetry?: () => void;
  /** Renames the session; see SessionTitle. */
  onRename?: (title: string) => Promise<boolean>;
  renaming?: Sending;
  onRenameReset?: () => void;
  renameDisabled?: boolean;
}

/** The active session as a document: the user's and the agent's words, tool cards, and permission requests to answer. */
export function Transcript({ title, untitled, view, answers = {}, onDecide, onRetry, onRename, renaming, onRenameReset, renameDisabled }: TranscriptProps) {
  const { transcript, stream } = view;
  const scroller = useRef<HTMLDivElement>(null);
  // Whether the reader is at the end; new content keeps them there, and
  // leaves them alone when they have scrolled back to read.
  const atEnd = useRef(true);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && atEnd.current) el.scrollTop = el.scrollHeight;
  });

  const onScroll = () => {
    const el = scroller.current;
    if (el) atEnd.current = el.scrollHeight - el.scrollTop - el.clientHeight < el.clientHeight / 8;
  };

  // Tools whose request waits for the user: their cards say so rather than "Running".
  const awaiting = new Set(
    transcript.items.flatMap((i) => (i.kind === "permission" && isOpen(i) ? [i.request.tool_use_id] : [])),
  );
  // Answers go to Core whenever the window is connected, whether or not the
  // event subscription is up: a failed subscribe must not strand a request.
  const live = stream.state !== "offline";

  return (
    <section className="transcript" aria-label="Transcript">
      <header className="transcript-header">
        <SessionTitle title={title} untitled={untitled} onRename={onRename} renaming={renaming} onReset={onRenameReset} disabled={renameDisabled}>
          <span className="transcript-state" data-needs-you={transcript.state === "waiting_user" || undefined}>
            {stateText(transcript.state)}
          </span>
        </SessionTitle>
      </header>
      <div className="transcript-scroll" ref={scroller} onScroll={onScroll}>
        <div className="transcript-page">
          {stream.state === "failed" && (
            <div className="transcript-actions">
              <p className="problem" role="alert">
                Not receiving this session's events: {stream.message}
              </p>
              {onRetry && <Button onClick={onRetry}>Retry</Button>}
            </div>
          )}
          {stream.state === "offline" && <p className="transcript-note">Reconnecting to Core…</p>}
          {transcript.items.length === 0 && (stream.state === "subscribing" || stream.state === "live") && (
            <p className="transcript-note">{stream.state === "subscribing" ? "Loading…" : "Nothing yet. Write to the agent below."}</p>
          )}
          {transcript.items.map((item) => (
            <Block
              key={item.key}
              item={item}
              answer={item.kind === "permission" && isOpen(item) ? answers[item.request.request_id] : undefined}
              waiting={item.kind === "tool" && awaiting.has(item.key.slice("tool:".length))}
              live={live}
              onDecide={onDecide}
            />
          ))}
          {transcript.refused && (
            <p className="problem" role="alert">
              {transcript.refused}
            </p>
          )}
        </div>
      </div>
    </section>
  );
}

interface BlockProps {
  item: Item;
  answer?: Answer;
  /** A tool card whose permission request waits for the user. */
  waiting: boolean;
  /** Whether answers can reach Core now. */
  live: boolean;
  onDecide: Decide;
}

/**
 * What came with a user's message, above its words (FLOW-SEND-MESSAGE-R11; DESIGN.md, Layout): an image as the image;
 * a file or folder by its name, focusable, its path the agent was given as its name and tooltip.
 */
function MessageAttachments({ attachments }: { attachments: readonly MessageAttachment[] }) {
  return (
    <ul className="transcript-attachments" aria-label={t("transcript.attachment.listLabel")}>
      {attachments.map((a, i) =>
        a.kind === "image" ? (
          <AttachedImage key={i} image={a} />
        ) : (
          <AttachmentChip key={i} kind={a.kind} name={pathName(a.path)} path={a.path} />
        ),
      )}
    </ul>
  );
}

/**
 * An image as itself, named "Attached image". One of a kind the window can't show, or that fails to load, is the
 * attachment with an image icon by that name.
 */
function AttachedImage({ image }: { image: Extract<MessageAttachment, { kind: "image" }> }) {
  const [failed, setFailed] = useState(false);
  const label = t("transcript.attachment.imageLabel");
  if (failed || !image.media_type.toLowerCase().startsWith("image/")) return <AttachmentChip kind="image" name={label} />;
  return (
    <li className="transcript-attachment-image">
      <img src={`data:${image.media_type};base64,${image.data}`} alt={label} onError={() => setFailed(true)} />
    </li>
  );
}

/**
 * A user's message (DESIGN.md, Layout): the bubble shows once its images are decoded, so nothing in it moves. Kept
 * hidden, at its place, until then; an image that fails to decode lets it show, falling back to its attachment.
 */
function UserBubble({ item }: { item: Extract<Item, { kind: "user" }> }) {
  const bubble = useRef<HTMLDivElement>(null);
  const hasImages = item.attachments?.some((a) => a.kind === "image") ?? false;
  const [decoded, setDecoded] = useState(!hasImages);
  useLayoutEffect(() => {
    if (decoded) return;
    let live = true;
    const images = Array.from(bubble.current?.querySelectorAll("img") ?? []);
    Promise.all(images.map((img) => (typeof img.decode === "function" ? img.decode().catch(() => {}) : undefined))).then(() => {
      if (live) setDecoded(true);
    });
    return () => {
      live = false;
    };
  }, [decoded]);
  return (
    // A bubble with no label: its place says who wrote it, and assistive technology hears "You" (DESIGN.md, Layout).
    <div ref={bubble} className="transcript-user" role="group" aria-label="You" data-decoding={!decoded || undefined}>
      {item.attachments && <MessageAttachments attachments={item.attachments} />}
      {item.text !== "" && <p className="transcript-text">{item.text}</p>}
    </div>
  );
}

/**
 * One block. Memoised: items keep their identity until their own event
 * changes them, so a streamed delta re-renders one block, not the page.
 */
const Block = memo(function Block({ item, answer, waiting, live, onDecide }: BlockProps) {
  switch (item.kind) {
    case "user":
      return <UserBubble item={item} />;
    case "assistant":
      return <p className="transcript-text">{item.text}</p>;
    case "thinking":
      return (
        <p className="transcript-note">
          {item.final ? "Thought" : "Thinking…"}
          {item.summary ? `: ${item.summary}` : ""}
        </p>
      );
    case "tool":
      return <ToolCard item={item} waiting={waiting} />;
    case "permission":
      return <PermissionCard item={item} answer={answer} live={live} onDecide={onDecide} />;
    case "question":
      return (
        <div className="transcript-card">
          <span className="transcript-card-title">{item.answered ? "Question, answered" : "Question"}</span>
          {item.questions.map((q) => (
            <p key={q.id} className="transcript-card-detail">
              <Revealed text={q.text} />
            </p>
          ))}
        </div>
      );
    case "problem":
      return (
        <p className="problem" role="alert">
          {item.message}
        </p>
      );
    case "notice":
      return <p className="transcript-note">{item.message}</p>;
    case "ended":
      return <p className="transcript-ended">{item.reason}</p>;
  }
});

/** A card's state: the shared glyph and a word. */
function Status({ tone, children }: { tone?: "positive" | "pending" | "danger" | "attention"; children: ReactNode }) {
  return (
    <span className="transcript-card-status transcript-status">
      <span className="glyph" data-tone={tone} aria-hidden="true" />
      {children}
    </span>
  );
}

function ToolCard({ item, waiting }: { item: Extract<Item, { kind: "tool" }>; waiting: boolean }) {
  const status = item.result ? (
    item.result.isError ? (
      <Status tone="danger">Failed</Status>
    ) : (
      <Status tone="positive">Done</Status>
    )
  ) : waiting ? (
    <Status tone="attention">Waiting for you</Status>
  ) : (
    <Status tone="pending">{item.progress === undefined ? "Running…" : <Revealed text={item.progress} />}</Status>
  );
  return (
    <div className="transcript-card">
      <div className="transcript-card-head">
        <span className="transcript-card-title">
          <Revealed text={item.name} />
        </span>
        {status}
      </div>
      <code className="transcript-input mono">
        <Revealed text={inputSummary(item.input)} />
      </code>
      {item.result && item.result.content !== "" && <pre className="transcript-output mono">{item.result.content}</pre>}
    </div>
  );
}

function PermissionCard({
  item,
  answer,
  live,
  onDecide,
}: {
  item: Extract<Item, { kind: "permission" }>;
  answer?: Answer;
  live: boolean;
  onDecide: Decide;
}) {
  const { request, decision } = item;
  const name = request.display_name ?? request.tool_name;
  const open = isOpen(item);
  return (
    // Focusable outside the tab order, so focus moves to the card as an answer makes its answers unavailable.
    <section tabIndex={-1} className="transcript-card transcript-permission" data-open={open || undefined} aria-label={`Permission: ${name}`}>
      <div className="transcript-card-head">
        <span className="transcript-card-title">
          {open ? "Allow " : ""}
          <Revealed text={name} />
          {open ? "?" : ""}
          {/* The tool's own name, when the backend shows another, so the user knows what they approve. */}
          {name !== request.tool_name && (
            <span className="transcript-card-status mono">
              {" "}
              <Revealed text={request.tool_name} />
            </span>
          )}
        </span>
        {decision && (
          <Status tone={decision.behavior === "allow" ? "positive" : undefined}>{decisionText(decision.behavior, decision.decided_by)}</Status>
        )}
        {item.unanswered && <Status>Not answered</Status>}
      </div>
      {request.description && (
        <p className="transcript-card-detail">
          <Revealed text={request.description} />
        </p>
      )}
      {/* Every field of the input, whole while the request is open: the user approves exactly what will run. */}
      <PermissionInput request={request} />
      {request.blocked_path && (
        <p className="transcript-card-detail">
          Path:{" "}
          <span className="mono">
            <Revealed text={request.blocked_path} />
          </span>
        </p>
      )}
      {/* The answers the shelf would offer, with the same primary (FLOW-APPROVE-PERMISSION-R27). None is focused by
          default, and Enter never approves. The transcript says it is reconnecting above, once. */}
      {open && (
        <PermissionAnswers
          request={request}
          answer={answer}
          live={live}
          reconnectingNote={false}
          onDecide={(decision) => onDecide(request.request_id, decision)}
        />
      )}
    </section>
  );
}

/**
 * A permission request's input, field by field, with every hidden character marked: the transcript's permission
 * card and the shelf both show it so (FLOW-APPROVE-PERMISSION-R11). The description is shown once, as the request's
 * own detail, not again as a field.
 */
export function PermissionInput({ request }: { request: PermissionRequested }) {
  const fields = Object.entries(request.input as Record<string, unknown>).filter(
    ([key, value]) => !(key === "description" && value === request.description),
  );
  return (
    <dl className="transcript-fields">
      {fields.map(([key, value]) => (
        <div key={key} className="transcript-field">
          <dt>
            <Revealed text={key} />
          </dt>
          <dd className="mono">
            <Revealed text={typeof value === "string" ? value : JSON.stringify(value, null, 2)} />
          </dd>
        </div>
      ))}
    </dl>
  );
}

/**
 * Characters that change what text appears to say without showing
 * themselves: every control (other than tab, newline, and a carriage return
 * before a newline), every format character (bidirectional marks, overrides
 * and isolates, zero-width characters, tag characters), every default-
 * ignorable code point (fillers, variation selectors), line and paragraph
 * separators, and the blank braille pattern. An agent could use them to make
 * a command read as something else.
 */
const HIDDEN =
  /\r(?!\n)|(?![\t\n\r])[\p{Cc}\p{Cf}\p{Default_Ignorable_Code_Point}\p{Zl}\p{Zp}\u2800]/gu;

/** Letters of the scripts whose spelling uses joiners. Never Latin: there a joiner only hides a split word. */
const JOINING = /^[\p{Script=Arabic}\p{Script=Syriac}\p{Script=Devanagari}\p{Script=Bengali}\p{Script=Gurmukhi}\p{Script=Gujarati}\p{Script=Oriya}\p{Script=Tamil}\p{Script=Telugu}\p{Script=Kannada}\p{Script=Malayalam}\p{Script=Sinhala}]$/u;

/**
 * Joiners and the emoji presentation selector do real work inside emoji
 * (a warning sign drawn as emoji, family sequences) and between letters of
 * scripts that need them (Arabic, Indic); there they hide nothing and are
 * left as they are. Anywhere else, they are marked.
 */
function benign(text: string, at: number, hidden: string): boolean {
  const code = hidden.codePointAt(0);
  const prev = text.slice(Math.max(0, at - 2), at);
  const after = text.slice(at + hidden.length, at + hidden.length + 2);
  if (code === 0xfe0f) return /\p{Extended_Pictographic}$/u.test(prev);
  if (code === 0x200d && /(\p{Extended_Pictographic}|\p{Emoji_Modifier}|\uFE0F)$/u.test(prev)) {
    return /^\p{Extended_Pictographic}/u.test(after);
  }
  if (code === 0x200c || code === 0x200d) {
    // The letter before, past any marks on it (harakat, vowel signs).
    const base = text.slice(0, at).replace(/\p{M}+$/u, "").slice(-1);
    return JOINING.test(base) && JOINING.test(after.slice(0, 1));
  }
  return false;
}

/** Past this many, hidden characters are counted rather than each marked, so a flood of them cannot stall the window. */
const MARKED = 64;

/** `text` with every hidden character shown as a marked code point, so what the user reads is what the agent sent. */
export function Revealed({ text }: { text: string }) {
  const parts: ReactNode[] = [];
  let last = 0;
  let marked = 0;
  for (const match of text.matchAll(HIDDEN)) {
    const at = match.index;
    if (benign(text, at, match[0])) continue;
    if (marked === MARKED) {
      // Past the cap each hidden character stays where it was, as the
      // replacement character in one plain run of text (cheap however many
      // there are), and a note at the end says how many there were.
      const rest = text.slice(last);
      let more = 0;
      parts.push(
        rest.replace(HIDDEN, () => {
          more++;
          return "\uFFFD";
        }),
        <span key="more" className="transcript-hidden" title="Invisible characters">
          {more} more invisible characters, each shown as {"\uFFFD"}
        </span>,
      );
      return <>{parts}</>;
    }
    marked++;
    if (at > last) parts.push(text.slice(last, at));
    const code = (match[0].codePointAt(0) ?? 0).toString(16).toUpperCase().padStart(4, "0");
    parts.push(
      <span key={at} className="transcript-hidden" title="An invisible character">
        U+{code}
      </span>,
    );
    last = at + match[0].length;
  }
  if (parts.length === 0) return text;
  if (last < text.length) parts.push(text.slice(last));
  return <>{parts}</>;
}

/** A tool card's one-line view of its arguments: its command or path when it has one, else them all as JSON. */
function inputSummary(input: ToolInput): string {
  const fields = input as Record<string, unknown>;
  if (typeof fields.command === "string") return fields.command;
  if (typeof fields.file_path === "string") return fields.file_path;
  return JSON.stringify(input);
}

function decisionText(behavior: "allow" | "deny", by: "user" | "rule" | "backend" | "timeout"): string {
  const what = behavior === "allow" ? "Allowed" : "Denied";
  return by === "user" ? what : `${what} (${by === "timeout" ? "timed out" : `by ${by}`})`;
}
