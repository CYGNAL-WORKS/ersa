// A tool card, a diff card and the thinking row (FLOW-READ-TRANSCRIPT-R06 to R13, R16; docs/design/TRANSCRIPT.md, Tool
// cards, Diff cards and Thinking): one row that says the call's state, tool, target and detail, and opens to its input,
// its subagent's calls and its output, or, for a call that changes files, to the change itself.

import { Check, ChevronRight, LoaderCircle } from "lucide-react";
import { type ReactNode, useEffect, useId, useMemo, useState } from "react";
import { t } from "../catalog";
import type { ToolInput } from "../protocol/agent-events.gen";
import { type DiffLine, type FileDiff, diffCounts, fileDiffs } from "../session/diff";
import { type ShownTarget, type ToolState, shownTarget, toolDetail, toolState } from "../session/toolCard";
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
  stopped: () => t("transcript.tool.stoppedStatus"),
};

/**
 * The state mark, in one `size-icon` place so the row never moves: the turning ring while the call runs, a caution
 * glyph while it waits for the user, a check once done, a danger glyph once failed, the neutral glyph in ink-faint once
 * stopped (R16), or nothing (a thought). Keyed by state, so each new mark fades in where the last one was.
 */
export function StateMark({ state }: { state: ToolState | "none" }) {
  return (
    <span className="tool-mark" data-state={state} aria-hidden="true">
      <span key={state} className="tool-mark-face">
        {state === "running" && <Icon icon={LoaderCircle} />}
        {state === "done" && <Icon icon={Check} />}
        {(state === "waiting" || state === "failed") && <span className="glyph" data-tone={state === "waiting" ? "caution" : "danger"} />}
        {/* The shared glyph's own tone is the neutral ink-faint. */}
        {state === "stopped" && <span className="glyph" />}
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

/**
 * One tool call as a card, closed at first (R07), with its subagent's calls inside it (R10). A call whose event carries
 * `edits` is a diff card (R09): the same row with the lines added and removed as its detail, open at first to the
 * change, one diff per file.
 */
export function ToolCard({
  item,
  context,
  force,
  initialOpen,
}: {
  item: Tool;
  context: ToolContext;
  /** For the gallery: a state shown without a pointer, and a card shown open. */
  force?: "hover" | "pressed" | "focus";
  /** Whether the card starts open: a diff card does and a tool card doesn't, unless this says otherwise. */
  initialOpen?: boolean;
}) {
  const diffs = useMemo(() => (item.edits ? fileDiffs(item.edits) : undefined), [item.edits]);
  const [open, setOpen] = useState(initialOpen ?? diffs !== undefined);
  const id = useId();
  const state = toolState(item, context.awaiting.has(item.key));
  // A diff card shows its counts in place of a clock (TRANSCRIPT.md, Diff cards), so it never ticks.
  const unfinished = !diffs && !item.result && item.live && !item.stopped && item.progress === undefined;
  const now = useNow(unfinished, context.now);
  const counts = diffs ? diffCounts(diffs) : undefined;
  const target = diffs ? diffTarget(diffs, context.workspace) : item.target === undefined ? undefined : shownTarget(item.target, context.workspace);
  const calls = context.calls.get(item.key) ?? [];
  // Heard: the tool, its target, its state, and a diff card's counts; whether it is open is the button's own (R12).
  const row = String(
    target === undefined
      ? t("transcript.tool.rowNoTargetLabel", { tool: item.name, state: stateName[state]() })
      : t("transcript.tool.rowLabel", { tool: item.name, target: target.text, state: stateName[state]() }),
  );
  const name = counts
    ? t("transcript.diff.countsLabel", {
        row,
        added: String(t("transcript.diff.addedPart", { count: counts.added })),
        removed: String(t("transcript.diff.removedPart", { count: counts.removed })),
      })
    : row;
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
          <Detail item={item} state={state} counts={counts} now={now} />
        </span>
        <Chevron />
      </button>
      <OpenPart id={id} open={open}>
        {diffs ? <DiffChange diffs={diffs} item={item} workspace={context.workspace} /> : <ToolOpenBody item={item} calls={calls} context={context} />}
      </OpenPart>
    </div>
  );
}

/** An open tool card: its input, its subagent's calls, then its output or error. */
function ToolOpenBody({ item, calls, context }: { item: Tool; calls: readonly Tool[]; context: ToolContext }) {
  return (
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
  );
}

/**
 * The row's detail: "Failed" in danger, "Stopped" in ink-faint (R16), a diff card's counts, else the progress, the
 * clock or the duration.
 */
function Detail({ item, state, counts, now }: { item: Tool; state: ToolState; counts?: { added: number; removed: number }; now: number }) {
  if (state === "failed") return t("transcript.tool.failedStatus");
  if (state === "stopped") return t("transcript.tool.stoppedStatus");
  if (counts) {
    // Each figure after its sign, so the counts never depend on color alone.
    return (
      <span className="diff-counts">
        <span className="diff-added">{t("transcript.diff.addedDetail", { count: counts.added })}</span>
        <span className="diff-removed">{t("transcript.diff.removedDetail", { count: counts.removed })}</span>
      </span>
    );
  }
  const detail = toolDetail(item, now);
  return detail === undefined ? null : <Revealed text={detail} />;
}

/** A diff card's target: the first file's path, and how many more files the call changes. Always a path. */
function diffTarget(diffs: readonly FileDiff[], workspace: string | undefined): ShownTarget | undefined {
  const first = diffs[0];
  if (!first) return undefined;
  const path = shownTarget(first.path, workspace).text;
  const more = diffs.length - 1;
  return { text: more > 0 ? String(t("transcript.diff.filesLabel", { path, count: more })) : path, path: true };
}

/** A line's sign as the gutter shows it: a removed line takes the minus sign, not the hyphen. */
const signShown: Record<DiffLine["sign"], string> = { "+": "+", "-": "−", " ": " " };
const signName: Record<DiffLine["sign"], string> = { "+": "added", "-": "removed", " ": "kept" };

/**
 * The change, one diff per file (R09): each line its sign in a gutter and its text, an added or removed line on its
 * tint. A file with an edit made at every occurrence of its text says so above its lines (R13). With several files,
 * each file's path heads its diff. A failed call keeps its change and adds its error beneath it.
 */
function DiffChange({ diffs, item, workspace }: { diffs: readonly FileDiff[]; item: Tool; workspace: string | undefined }) {
  return (
    <div className="diff-body">
      {diffs.map((file) => (
        <section key={file.path} className="diff-file">
          {diffs.length > 1 && (
            <span className="diff-path mono">
              <Revealed text={shownTarget(file.path, workspace).text} />
            </span>
          )}
          <div className="diff-change">
            {file.everyOccurrence && <p className="diff-note">{t("transcript.diff.everyOccurrenceLabel")}</p>}
            <div className="diff-scroll">
              <div className="diff-lines mono">
                {file.lines.map((line, index) => (
                  // A line's place is what tells it apart (the same text can be removed and added), and lines never
                  // reorder.
                  // biome-ignore lint/suspicious/noArrayIndexKey: see above
                  <div key={index} className="diff-line" data-sign={signName[line.sign]}>
                    <span className="diff-sign">{signShown[line.sign]}</span>
                    <span className="diff-text">
                      <Revealed text={line.text} />
                    </span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </section>
      ))}
      {item.result?.isError && (
        <div className="tool-open-body">
          <span className="tool-label">{t("transcript.tool.errorLabel")}</span>
          {item.result.content !== "" ? (
            <pre className="tool-output mono">{item.result.content}</pre>
          ) : (
            <p className="tool-empty">{t("transcript.tool.noOutputLabel")}</p>
          )}
        </div>
      )}
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
