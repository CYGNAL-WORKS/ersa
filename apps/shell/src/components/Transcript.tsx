import { useLayoutEffect, useRef } from "react";
import type { ToolInput } from "../protocol/agent-events.gen";
import type { SessionView, Sending } from "../session/follow";
import { stateText } from "../session/states";
import { type Item, isOpen } from "../session/transcript";
import { Button } from "./Button";
import "./Transcript.css";

interface TranscriptProps {
  title: string;
  view: SessionView;
  onDecide: (requestId: string, behavior: "allow" | "deny") => void;
}

/** The active session as a document: the user's and the agent's words, tool cards, and permission requests to answer. */
export function Transcript({ title, view, onDecide }: TranscriptProps) {
  const { transcript, stream, answers } = view;
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

  return (
    <section className="transcript" aria-label="Transcript">
      <header className="transcript-header">
        <h1 className="transcript-title">{title}</h1>
        <span className="transcript-state">{stateText(transcript.state)}</span>
      </header>
      <div className="transcript-scroll" ref={scroller} onScroll={onScroll}>
        <div className="transcript-page">
          {stream.state === "failed" && (
            <p className="problem" role="alert">
              Not receiving this session's events: {stream.message}
            </p>
          )}
          {transcript.items.length === 0 && stream.state !== "failed" && (
            <p className="transcript-note">{stream.state === "subscribing" ? "Loading…" : "Nothing yet. Write to the agent below."}</p>
          )}
          {transcript.items.map((item) => (
            <Block key={item.key} item={item} answers={answers} onDecide={onDecide} />
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

function Block({ item, answers, onDecide }: { item: Item; answers: SessionView["answers"]; onDecide: TranscriptProps["onDecide"] }) {
  switch (item.kind) {
    case "user":
      return (
        <div className="transcript-user">
          <span className="transcript-who">You</span>
          <p className="transcript-text">{item.text}</p>
        </div>
      );
    case "assistant":
      return <p className="transcript-text">{item.text}</p>;
    case "thinking":
      return <p className="transcript-note">{item.final ? "Thought" : "Thinking…"}{item.summary ? `: ${item.summary}` : ""}</p>;
    case "tool":
      return <ToolCard item={item} />;
    case "permission":
      return <PermissionCard item={item} answer={isOpen(item) ? answers[item.request.request_id] : undefined} onDecide={onDecide} />;
    case "question":
      return (
        <div className="transcript-card">
          <span className="transcript-card-title">{item.answered ? "Question, answered" : "Question"}</span>
          {item.questions.map((q) => (
            <p key={q.id} className="transcript-card-detail">
              {q.text}
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
}

function ToolCard({ item }: { item: Extract<Item, { kind: "tool" }> }) {
  return (
    <div className="transcript-card" data-error={item.result?.isError || undefined}>
      <div className="transcript-card-head">
        <span className="transcript-card-title">{item.name}</span>
        <span className="transcript-card-status">{item.result ? (item.result.isError ? "Failed" : "Done") : (item.progress ?? "Running…")}</span>
      </div>
      <code className="transcript-input mono">{inputSummary(item.input)}</code>
      {item.result && item.result.content !== "" && <pre className="transcript-output mono">{item.result.content}</pre>}
    </div>
  );
}

function PermissionCard({
  item,
  answer,
  onDecide,
}: {
  item: Extract<Item, { kind: "permission" }>;
  answer?: Sending;
  onDecide: TranscriptProps["onDecide"];
}) {
  const { request, decision } = item;
  const name = request.display_name ?? request.tool_name;
  const sending = answer?.state === "sending";
  return (
    <section className="transcript-card transcript-permission" data-open={!decision || undefined} aria-label={`Permission: ${name}`}>
      <div className="transcript-card-head">
        <span className="transcript-card-title">{decision ? name : `Allow ${name}?`}</span>
        {decision && <span className="transcript-card-status">{decisionText(decision.behavior, decision.decided_by)}</span>}
      </div>
      {request.description && <p className="transcript-card-detail">{request.description}</p>}
      <code className="transcript-input mono">{inputSummary(request.input)}</code>
      {request.blocked_path && (
        <p className="transcript-card-detail">
          Path: <span className="mono">{request.blocked_path}</span>
        </p>
      )}
      {!decision && (
        <div className="transcript-actions">
          <Button onClick={() => onDecide(request.request_id, "allow")} disabled={sending}>
            Allow
          </Button>
          <Button onClick={() => onDecide(request.request_id, "deny")} disabled={sending}>
            Deny
          </Button>
          {sending && <span className="transcript-card-status">Sending…</span>}
        </div>
      )}
      {!decision && answer?.state === "failed" && (
        <p className="problem" role="alert">
          {answer.message}
        </p>
      )}
    </section>
  );
}

/** A tool's arguments on one line: its command when it has one, else its arguments as JSON. */
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
