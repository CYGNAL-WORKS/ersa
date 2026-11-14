// A tool card and the thinking row (FLOW-READ-TRANSCRIPT-R06, R07, R10, R11, R12; docs/design/TRANSCRIPT.md, Tool
// cards and Thinking): one row that says the call's state, tool, target and detail, and opens to its input, its
// subagent's calls and its output.

import { Check, ChevronRight, LoaderCircle } from "lucide-react";
import { type ReactNode, useEffect, useId, useState } from "react";
import { t } from "../catalog";
import type { ToolInput } from "../protocol/agent-events.gen";
import { type ToolState, shownTarget, toolDetail, toolState } from "../session/toolCard";
import type { Item } from "../session/transcript";
import { Icon } from "./Icon";
import { Revealed } from "./Revealed";
import "./ToolCard.css";

type Tool = Extract<Item, { kind: "tool" }>;

/** What every card in one transcript shares: whose permission requests are open, a subagent's calls, the folder. */
export interface ToolContext {
  /** Keys of the calls whose permission request waits for the user. */
  awaiting: ReadonlySet<string>;
  /** The calls each call's subagent made, by the parent's key, in the order they were asked for. */
  calls: ReadonlyMap<string, readonly Tool[]>;
  /** The session's workspace folder, against which a target that is a path shows relative (R08). */
  workspace?: string;
  /** Now, for the gallery; a live card otherwise reads the clock itself. */
  now?: number;
}

/** The time, moved on each second while `ticking`, so a running call's clock keeps true. */
function useNow(ticking: boolean, fixed?: number): number {
  const [now, setNow] = useState(() => fixed ?? Date.now());
  useEffect(() => {
    if (!ticking || fixed !== undefined) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [ticking, fixed]);
  return fixed ?? now;
}

const stateName: Record<ToolState, () => string> = {
  running: () => t("transcript.tool.runningStatus"),
  waiting: () => t("transcript.tool.waitingLabel"),
  done: () => t("transcript.tool.doneStatus"),
  failed: () => t("transcript.tool.failedStatus"),
};

/**
 * The state mark, in one `size-icon` place so the row never moves: the turning ring while the call runs, a caution
 * glyph while it waits for the user, a check once done, a danger glyph once failed, or nothing (a thought). Keyed by
 * state, so each new mark fades in where the last one was.
 */
export function StateMark({ state }: { state: ToolState | "none" }) {
  return (
    <span className="tool-mark" data-state={state} aria-hidden="true">
      <span key={state} className="tool-mark-face">
        {state === "running" && <Icon icon={LoaderCircle} />}
        {state === "done" && <Icon icon={Check} />}
        {(state === "waiting" || state === "failed") && <span className="glyph" data-tone={state === "waiting" ? "caution" : "danger"} />}
      </span>
    </span>
  );
}

/**
 * What opens beneath a row: closed, it takes no room and nothing in it is reachable; it grows open and shrinks closed
 * over motion-settle, as the rail's groups do, so what follows slides rather than jumps.
 */
function OpenPart({ id, open, children }: { id: string; open: boolean; children: ReactNode }) {
  return (
    <div id={id} className="tool-open" data-open={open || undefined} inert={!open || undefined}>
      <div className="tool-open-inner">{children}</div>
    </div>
  );
}

/** The chevron after the detail, turned a quarter when the card is open. */
function Chevron() {
  return (
    <span className="tool-chevron" aria-hidden="true">
      <Icon icon={ChevronRight} size="icon-xs" />
    </span>
  );
}

/**
 * A tool's input, field by field, every hidden character shown. A tool card sets each name above its value; the
 * permission card passes its own classes to set them side by side.
 */
export function InputFields({
  input,
  omit,
  className = "tool-fields",
  fieldClassName = "tool-field",
}: {
  input: ToolInput;
  omit?: (key: string, value: unknown) => boolean;
  className?: string;
  fieldClassName?: string;
}) {
  const fields = Object.entries(input as Record<string, unknown>).filter(([key, value]) => !omit?.(key, value));
  return (
    <dl className={className}>
      {fields.map(([key, value]) => (
        <div key={key} className={fieldClassName}>
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

/** One tool call as a card, closed at first (R07), with its subagent's calls inside it (R10). */
export function ToolCard({
  item,
  context,
  force,
  initialOpen = false,
}: {
  item: Tool;
  context: ToolContext;
  /** For the gallery: a state shown without a pointer, and a card shown open. */
  force?: "hover" | "pressed" | "focus";
  initialOpen?: boolean;
}) {
  const [open, setOpen] = useState(initialOpen);
  const id = useId();
  const state = toolState(item, context.awaiting.has(item.key));
  const unfinished = !item.result && item.live && !item.stopped && item.progress === undefined;
  const now = useNow(unfinished, context.now);
  const detail = state === "failed" ? t("transcript.tool.failedStatus") : toolDetail(item, now);
  const target = item.target === undefined ? undefined : shownTarget(item.target, context.workspace);
  const calls = context.calls.get(item.key) ?? [];
  // Heard: the tool, its target, its state; whether it is open is the button's own (R12).
  const name =
    target === undefined
      ? t("transcript.tool.rowNoTargetLabel", { tool: item.name, state: stateName[state]() })
      : t("transcript.tool.rowLabel", { tool: item.name, target: target.text, state: stateName[state]() });
  return (
    <div className="tool-card">
      <button
        type="button"
        className="tool-row"
        aria-expanded={open}
        aria-controls={id}
        aria-label={name}
        data-force={force}
        onClick={() => setOpen((was) => !was)}
      >
        <StateMark state={state} />
        <span className="tool-name">
          <Revealed text={item.name} />
        </span>
        {target && (
          <span className="tool-target mono" data-path={target.path || undefined}>
            {/* A path is cut at its start, so the file's own name stays in view: the box runs right to left, the
                text inside it left to right. */}
            <bdi>
              <Revealed text={target.text} />
            </bdi>
          </span>
        )}
        <span className="tool-detail" data-failed={state === "failed" || undefined}>
          {detail !== undefined && (state === "failed" ? detail : <Revealed text={detail} />)}
        </span>
        <Chevron />
      </button>
      <OpenPart id={id} open={open}>
        <div className="tool-open-body">
          {/* Input, then Output or Error, each under its label whatever it holds (TRANSCRIPT.md, Tool cards); a part
              with nothing in it says so in quiet words, rather than showing an empty box. */}
          <span className="tool-label">{t("transcript.tool.inputLabel")}</span>
          {Object.keys(item.input).length > 0 ? <InputFields input={item.input} /> : <p className="tool-empty">{t("transcript.tool.noInputLabel")}</p>}

          {calls.length > 0 && (
            <div className="tool-calls">
              {calls.map((call) => (
                <ToolCard key={call.key} item={call} context={context} />
              ))}
            </div>
          )}
          {item.result && (
            <>
              <span className="tool-label">{item.result.isError ? t("transcript.tool.errorLabel") : t("transcript.tool.outputLabel")}</span>
              {item.result.content !== "" ? (
                <pre className="tool-output mono">{item.result.content}</pre>
              ) : (
                <p className="tool-empty">{t("transcript.tool.noOutputLabel")}</p>
              )}
            </>
          )}
        </div>
      </OpenPart>
    </div>
  );
}

/**
 * The agent's thinking (R11): a quiet row with no card, the ring while it thinks and its place kept empty once it has
 * thought. With a summary, the row opens to it as a card does; without one, there is nothing to open.
 */
export function ThinkingRow({
  item,
  force,
  initialOpen = false,
}: {
  item: Extract<Item, { kind: "thinking" }>;
  /** For the gallery, as a tool card's. */
  force?: "hover" | "pressed" | "focus";
  initialOpen?: boolean;
}) {
  const [open, setOpen] = useState(initialOpen);
  const id = useId();
  const words = (
    <>
      <StateMark state={item.final ? "none" : "running"} />
      <span>{item.final ? t("transcript.thoughtLabel") : t("transcript.thinkingProgress")}</span>
    </>
  );
  if (!item.summary) return <div className="thinking-row">{words}</div>;
  return (
    <div className="thinking-row">
      <button type="button" className="thinking-toggle" aria-expanded={open} aria-controls={id} data-force={force} onClick={() => setOpen((was) => !was)}>
        {words}
        <Chevron />
      </button>
      <OpenPart id={id} open={open}>
        <p className="thinking-summary">
          <Revealed text={item.summary} />
        </p>
      </OpenPart>
    </div>
  );
}
