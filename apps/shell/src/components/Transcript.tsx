import { Image } from "lucide-react";
import { type CSSProperties, type ReactNode, memo, useLayoutEffect, useMemo, useRef, useState } from "react";
import { t } from "../catalog";
import type { PermissionRequested } from "../protocol/agent-events.gen";
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
import { Revealed } from "./Revealed";
import { InputFields, ThinkingRow, ToolCard, type ToolContext } from "./ToolCard";
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
  /** The session's workspace folder, against which a tool card's path shows relative (FLOW-READ-TRANSCRIPT-R08). */
  workspace?: string;
  /** Now, for the gallery's running cards; the window reads the clock. */
  now?: number;
}

/** The active session as a document: the user's and the agent's words, tool cards, and permission requests to answer. */
export function Transcript({
  title,
  untitled,
  view,
  answers = {},
  onDecide,
  onAnswer,
  onRetry,
  onRename,
  renaming,
  onRenameReset,
  renameDisabled,
  onOpenLink,
  workspace,
  now,
}: TranscriptProps) {
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

  const tools = useToolContext(transcript.items, workspace, now);
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
          {transcript.items.map((item) =>
            // A subagent's call is shown inside the card of the call that started it (FLOW-READ-TRANSCRIPT-R10).
            item.kind === "tool" && tools.nested.has(item.key) ? null : (
            <Block
              key={item.key}
              item={item}
              answer={answerTo(item, answers)}
              tools={tools}
              live={live}
              onDecide={onDecide}
              onAnswer={onAnswer}
              onOpenLink={onOpenLink}
            />
            ),
          )}
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

/** What tool cards share, and which calls sit inside another's card rather than in the turn. */
interface Tools extends ToolContext {
  nested: ReadonlySet<string>;
}

/**
 * The tool cards' shared context, rebuilt only when a tool call or an open permission request changes: streamed text
 * leaves it as it was, so it doesn't re-render every card it reaches (Block is memoised on it).
 */
function useToolContext(items: readonly Item[], workspace: string | undefined, now: number | undefined): Tools {
  const tools = items.filter((i): i is Extract<Item, { kind: "tool" }> => i.kind === "tool");
  // Calls whose request waits for the user: their cards say so rather than running.
  const awaiting = items.flatMap((i) => (i.kind === "permission" && isOpen(i) ? [`tool:${i.request.tool_use_id}`] : []));
  const last = useRef<{ tools: readonly Item[]; awaiting: string; workspace?: string; now?: number; context: Tools }>(undefined);
  const key = awaiting.join(" ");
  const prev = last.current;
  if (prev && prev.awaiting === key && prev.workspace === workspace && prev.now === now && same(prev.tools, tools)) return prev.context;
  const known = new Set(tools.map((tool) => tool.key));
  const calls = new Map<string, Extract<Item, { kind: "tool" }>[]>();
  const nested = new Set<string>();
  for (const tool of tools) {
    // A call whose parent isn't in the transcript stays a block of the turn, rather than being lost.
    if (tool.parent === undefined || !known.has(tool.parent)) continue;
    nested.add(tool.key);
    calls.set(tool.parent, [...(calls.get(tool.parent) ?? []), tool]);
  }
  const context: Tools = { awaiting: new Set(awaiting), calls, nested, workspace, now };
  last.current = { tools, awaiting: key, workspace, now, context };
  return context;
}

const same = (a: readonly unknown[], b: readonly unknown[]) => a.length === b.length && a.every((x, i) => x === b[i]);

interface BlockProps {
  item: Item;
  answer?: Answer;
  /** What tool cards share; unchanged while only text streams. */
  tools: Tools;
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
const Block = memo(function Block({ item, answer, tools, live, onDecide, onAnswer, onOpenLink }: BlockProps) {
  switch (item.kind) {
    case "user":
      return <UserBubble item={item} />;
    case "assistant":
      // Formatted text, never HTML (FLOW-READ-TRANSCRIPT-R01).
      return <AgentText text={item.text} onOpenLink={onOpenLink} />;
    case "thinking":
      return <ThinkingRow item={item} />;
    case "tool":
      return <ToolCard item={item} context={tools} />;
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
function Status({ tone, children }: { tone?: "positive"; children: ReactNode }) {
  return (
    <span className="transcript-card-status transcript-status">
      <span className="glyph" data-tone={tone} aria-hidden="true" />
      {children}
    </span>
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
  return (
    <InputFields
      input={request.input}
      omit={(key, value) => key === "description" && value === request.description}
      className="transcript-fields"
      fieldClassName="transcript-field"
    />
  );
}

function decisionText(behavior: "allow" | "deny", by: "user" | "rule" | "backend" | "timeout"): string {
  const decision = behavior === "allow" ? t("transcript.permission.allowedStatus") : t("transcript.permission.deniedStatus");
  if (by === "user") return decision;
  const how = by === "rule" ? "byRuleLabel" : by === "backend" ? "byBackendLabel" : "timedOutLabel";
  return t(`transcript.permission.${how}`, { decision });
}
