import { Image } from "lucide-react";
import { type CSSProperties, type ReactNode, memo, useLayoutEffect, useMemo, useRef, useState } from "react";
import { t } from "../catalog";
import type { PermissionRequested, ToolInput } from "../protocol/agent-events.gen";
import type { Answer, Decision, Reply } from "../session/answers";
import type { SessionView, Sending } from "../session/follow";
import { pathName } from "../session/attachments";
import { imageSize } from "../session/imageSize";
import { stateText } from "../session/states";
import { type Item, type MessageAttachment, isOpen, isOpenQuestion } from "../session/transcript";
import { Button } from "./Button";
import { AgentText, type OpenLink } from "./AgentText";
import { AttachmentChip } from "./AttachmentChip";
import { Icon } from "./Icon";
import { PermissionAnswers } from "./PermissionAnswers";
import type { Draft } from "../session/questionForm";
import { QuestionForm } from "./QuestionForm";
import { SessionTitle } from "./SessionTitle";
import "./Transcript.css";

type Decide = (requestId: string, decision: Decision) => void;
type AnswerQuestion = (requestId: string, reply: Reply) => void;

interface TranscriptProps {
  title: string;
  /** The title is a placeholder; see SessionTitle. */
  untitled?: boolean;
  view: SessionView;
  /** The session's answers on their way or refused, by request id (answers.ts). */
  answers?: Readonly<Record<string, Answer>>;
  /** Kept stable across renders, so streaming text does not re-render every block. */
  onDecide: Decide;
  /** Answers a question or declines it; kept stable, as `onDecide` is. */
  onAnswer: AnswerQuestion;
  /** Subscribes again after subscribing failed. */
  onRetry?: () => void;
  /** Renames the session; see SessionTitle. */
  onRename?: (title: string) => Promise<boolean>;
  renaming?: Sending;
  onRenameReset?: () => void;
  renameDisabled?: boolean;
  /** Opens a link in the agent's words through the Shell (FLOW-READ-TRANSCRIPT-R03); kept stable, as `onDecide` is. */
  onOpenLink?: OpenLink;
}

/** The active session as a document: the user's and the agent's words, tool cards, and permission requests to answer. */
export function Transcript({ title, untitled, view, answers = {}, onDecide, onAnswer, onRetry, onRename, renaming, onRenameReset, renameDisabled, onOpenLink }: TranscriptProps) {
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
    <section className="transcript" aria-label={t("transcript.name")}>
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
                {t("transcript.streamFailedLabel", { message: stream.message })}
              </p>
              {onRetry && <Button onClick={onRetry}>{t("transcript.retryButton")}</Button>}
            </div>
          )}
          {stream.state === "offline" && <p className="transcript-note">{t("transcript.reconnectingProgress")}</p>}
          {transcript.items.length === 0 && (stream.state === "subscribing" || stream.state === "live") && (
            <p className="transcript-note">{stream.state === "subscribing" ? t("transcript.loadingProgress") : t("transcript.nothingYetEmpty")}</p>
          )}
          {transcript.items.map((item) => (
            <Block
              key={item.key}
              item={item}
              answer={answerTo(item, answers)}
              waiting={item.kind === "tool" && awaiting.has(item.key.slice("tool:".length))}
              live={live}
              onDecide={onDecide}
              onAnswer={onAnswer}
              onOpenLink={onOpenLink}
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
  onAnswer: AnswerQuestion;
  onOpenLink?: OpenLink;
}

/** The answer on its way or refused for a request still open, if any. */
function answerTo(item: Item, answers: Readonly<Record<string, Answer>>): Answer | undefined {
  if (item.kind === "permission" && isOpen(item)) return answers[item.request.request_id];
  if (item.kind === "question" && isOpenQuestion(item)) return answers[item.requestId];
  return undefined;
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
 * An image as itself, named "Attached image" (DESIGN.md, Layout). Its place takes its final size at once, from the
 * width and height its own header states (turned upright), capped at size-attachment-image with its proportions kept,
 * and is material-tile until the image is drawn, which then fades in, so nothing in the bubble moves. An image that
 * fails to draw after its place is set keeps that place and shows the image icon and "Attached image" in it. One of a
 * kind the window can't show, or that fails to load with no place set, is the attachment with an image icon.
 */
function AttachedImage({ image }: { image: Extract<MessageAttachment, { kind: "image" }> }) {
  const [state, setState] = useState<"drawing" | "drawn" | "failed">("drawing");
  const size = useMemo(() => imageSize(image.data), [image.data]);
  const label = t("transcript.attachment.imageLabel");
  if (!image.media_type.toLowerCase().startsWith("image/") || (state === "failed" && !size)) return <AttachmentChip kind="image" name={label} />;
  // The header's size reaches the CSS as custom properties; the cap and the proportions are the stylesheet's.
  const place = size && ({ "--image-width": `${size.width}px`, "--image-ratio": `${size.width} / ${size.height}`, "--image-aspect": size.width / size.height } as CSSProperties);
  return (
    <li className="transcript-attachment-image">
      <span className="attachment-image" style={place} data-sized={size ? "" : undefined} data-state={state}>
        {state === "failed" ? (
          <span className="attachment-image-failed">
            <Icon icon={Image} size="icon-sm" />
            {label}
          </span>
        ) : (
          <img src={`data:${image.media_type};base64,${image.data}`} alt={label} onLoad={() => setState("drawn")} onError={() => setState("failed")} />
        )}
      </span>
    </li>
  );
}

/** A user's message (DESIGN.md, Layout): its attachments above its words. */
function UserBubble({ item }: { item: Extract<Item, { kind: "user" }> }) {
  return (
    // A bubble with no label: its place says who wrote it, and assistive technology hears "You" (DESIGN.md, Layout).
    <div className="transcript-user" role="group" aria-label={t("transcript.userLabel")}>
      {item.attachments && <MessageAttachments attachments={item.attachments} />}
      {item.text !== "" && <p className="transcript-text">{item.text}</p>}
    </div>
  );
}

/**
 * One block. Memoised: items keep their identity until their own event
 * changes them, so a streamed delta re-renders one block, not the page.
 */
const Block = memo(function Block({ item, answer, waiting, live, onDecide, onAnswer, onOpenLink }: BlockProps) {
  switch (item.kind) {
    case "user":
      return <UserBubble item={item} />;
    case "assistant":
      // Formatted text, never HTML (FLOW-READ-TRANSCRIPT-R01).
      return <AgentText text={item.text} onOpenLink={onOpenLink} />;
    case "thinking":
      return (
        <p className="transcript-note">
          {item.final ? t("transcript.thoughtLabel") : t("transcript.thinkingProgress")}
          {item.summary ? `: ${item.summary}` : ""}
        </p>
      );
    case "tool":
      return <ToolCard item={item} waiting={waiting} />;
    case "permission":
      return <PermissionCard item={item} answer={answer} live={live} onDecide={onDecide} />;
    case "question":
      return <QuestionCard item={item} answer={answer} live={live} onAnswer={onAnswer} />;
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
      <Status tone="danger">{t("transcript.tool.failedStatus")}</Status>
    ) : (
      <Status tone="positive">{t("transcript.tool.doneStatus")}</Status>
    )
  ) : waiting ? (
    <Status tone="attention">{t("transcript.tool.waitingLabel")}</Status>
  ) : (
    <Status tone="pending">{item.progress === undefined ? t("transcript.tool.runningProgress") : <Revealed text={item.progress} />}</Status>
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
    <section tabIndex={-1} className="transcript-card transcript-permission" data-open={open || undefined} aria-label={t("transcript.permission.nameLabel", { tool: name })}>
      <div className="transcript-card-head">
        <span className="transcript-card-title">
          {open ? t("transcript.permission.openTitle", { tool: <Revealed text={name} /> }) : <Revealed text={name} />}
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
        {item.unanswered && <Status>{t("transcript.permission.unansweredStatus")}</Status>}
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
          {t("transcript.permission.pathLabel")}{" "}
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
 * A question: while nobody has answered it, the same answers the shelf would offer (FLOW-APPROVE-PERMISSION-R17,
 * R28); once answered, or once the session ended without an answer, its questions and what became of it.
 */
function QuestionCard({ item, answer, live, onAnswer }: { item: Extract<Item, { kind: "question" }>; answer?: Answer; live: boolean; onAnswer: AnswerQuestion }) {
  const open = isOpenQuestion(item);
  // The QuestionForm's choices so far: kept here, so they last as long as the card does.
  const [draft, setDraft] = useState<Draft>({});
  const title = item.declined ? "declinedTitle" : item.answered ? "answeredTitle" : "name";
  return (
    // Focusable outside the tab order, so focus moves to the card as an answer makes its answers unavailable.
    <section tabIndex={-1} className="transcript-card transcript-question" data-open={open || undefined} aria-label={t("transcript.question.name")}>
      <div className="transcript-card-head">
        <span className="transcript-card-title">{t(`transcript.question.${title}`)}</span>
        {item.unanswered && <Status>{t("transcript.question.unansweredStatus")}</Status>}
      </div>
      {open ? (
        // None is focused by default, and Enter never answers. The transcript says it is reconnecting above, once.
        <QuestionForm
          questions={item.questions}
          draft={draft}
          onDraft={setDraft}
          answer={answer}
          live={live}
          reconnectingNote={false}
          onAnswer={(reply) => onAnswer(item.requestId, reply)}
        />
      ) : (
        item.questions.map((q) => (
          <p key={q.id} className="transcript-card-detail">
            <Revealed text={q.text} />
          </p>
        ))
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
        <span key="more" className="transcript-hidden" title={t("transcript.hidden.manyLabel")}>
          {t("transcript.hidden.moreLabel", { count: more, mark: "\uFFFD" })}
        </span>,
      );
      return <>{parts}</>;
    }
    marked++;
    if (at > last) parts.push(text.slice(last, at));
    const code = (match[0].codePointAt(0) ?? 0).toString(16).toUpperCase().padStart(4, "0");
    parts.push(
      <span key={at} className="transcript-hidden" title={t("transcript.hidden.oneLabel")}>
        {t("transcript.hidden.codePointLabel", { code })}
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
  const decision = behavior === "allow" ? t("transcript.permission.allowedStatus") : t("transcript.permission.deniedStatus");
  if (by === "user") return decision;
  const how = by === "rule" ? "byRuleLabel" : by === "backend" ? "byBackendLabel" : "timedOutLabel";
  return t(`transcript.permission.${how}`, { decision });
}
