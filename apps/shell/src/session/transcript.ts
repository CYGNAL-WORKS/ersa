// The transcript: what the window shows of one session, derived only from
// its projected events (PROTO-AGENT-EVENTS), applied in `seq` order. Kept
// free of React so the tests feed it the replay server's captures.

import type {
  Envelope,
  PermissionDecided,
  PermissionRequested,
  QuestionAsked,
  SessionState,
  ToolInput,
} from "../protocol/agent-events.gen";

/** The event schema version this Interface renders (PROTO-AGENT-EVENTS, Versioning). */
export const EVENT_VERSION: Envelope["v"] = 1;

/** One block of the transcript. `key` is stable while the block changes, so it updates in place. */
export type Item =
  | { kind: "user"; key: string; text: string }
  /** The agent's words. Several events share a block and grow its text until one is `final`. */
  | { kind: "assistant"; key: string; text: string; final: boolean }
  | { kind: "thinking"; key: string; summary?: string; final: boolean }
  | {
      kind: "tool";
      key: string;
      name: string;
      input: ToolInput;
      progress?: string;
      result?: { content: string; isError: boolean };
    }
  /** A permission request, answered once `decision` is set; `unanswered` once the session ended without one. */
  | { kind: "permission"; key: string; request: PermissionRequested; decision?: PermissionDecided; unanswered?: true }
  /** Questions are answered from the shelf (M1); until then the transcript shows them. */
  | { kind: "question"; key: string; questions: QuestionAsked["questions"]; answered: boolean }
  /** Something went wrong: an `error` event, or a tool the backend denied with nobody to ask. */
  | { kind: "problem"; key: string; message: string }
  | { kind: "notice"; key: string; message: string }
  | { kind: "ended"; key: string; reason: string };

export interface Transcript {
  /** The last event applied; -1 before the first. Subscribing again starts after it. */
  lastSeq: number;
  items: Item[];
  /** The session's lifecycle state, as its latest `session.state` said. */
  state?: SessionState["state"];
  /** Set when an event came with a schema version this Interface does not know; nothing after it is applied. */
  refused?: string;
}

export const emptyTranscript: Transcript = { lastSeq: -1, items: [] };

/**
 * The transcript after `envelope`. Events apply only in order, each the one
 * after `lastSeq` (sequence numbers are gap-free, PROTO-AGENT-EVENTS-R01): one
 * at or before it was already applied (a subscription resumed with some
 * overlap), and one beyond it is a stray from an earlier subscription that
 * the current one will deliver again in its place. Either changes nothing.
 */
export function apply(transcript: Transcript, envelope: Envelope): Transcript {
  if (transcript.refused || envelope.seq !== transcript.lastSeq + 1) return transcript;
  if (envelope.v !== EVENT_VERSION) {
    return {
      ...transcript,
      refused: `This session's events use schema version ${envelope.v}; this window renders version ${EVENT_VERSION}.`,
    };
  }
  const next = { ...transcript, lastSeq: envelope.seq };
  const event = envelope.event;
  switch (event.type) {
    case "session.state":
      return { ...next, state: event.state };
    case "session.ended": {
      // Nobody can answer a request once the session has ended.
      const items = next.items.map((i) => (i.kind === "permission" && isOpen(i) ? { ...i, unanswered: true as const } : i));
      return add({ ...next, items }, { kind: "ended", key: `ended:${envelope.seq}`, reason: endedReason[event.reason] ?? "Ended." });
    }
    case "session.notice":
      return add(next, { kind: "notice", key: `notice:${envelope.seq}`, message: event.message });
    case "user.message":
      return add(next, { kind: "user", key: `user:${envelope.seq}`, text: event.text });
    case "assistant.text":
      return put(next, { kind: "assistant", key: `text:${event.block_id}`, text: event.text, final: event.final });
    case "assistant.thinking":
      return put(next, {
        kind: "thinking",
        key: `thinking:${event.block_id}`,
        summary: event.summary,
        final: event.final,
      });
    case "tool.requested":
      return put(next, {
        kind: "tool",
        key: `tool:${event.tool_use_id}`,
        name: event.display_name ?? event.name,
        input: event.input,
      });
    case "tool.progress":
      return patch(next, `tool:${event.tool_use_id}`, "tool", (item) => ({ ...item, progress: event.detail }));
    case "tool.result":
      return patch(next, `tool:${event.tool_use_id}`, "tool", (item) => ({
        ...item,
        result: { content: contentText(event.content), isError: event.is_error },
      }));
    case "permission.requested":
      return put(next, { kind: "permission", key: `permission:${event.request_id}`, request: event });
    case "permission.decided":
      return patch(next, `permission:${event.request_id}`, "permission", (item) => ({ ...item, decision: event }));
    case "permission.denied_unanswered":
      return add(next, { kind: "problem", key: `problem:${envelope.seq}`, message: event.message });
    case "question.asked":
      return put(next, { kind: "question", key: `question:${event.request_id}`, questions: event.questions, answered: false });
    case "question.answered":
      return patch(next, `question:${event.request_id}`, "question", (item) => ({ ...item, answered: true }));
    case "error":
      return add(next, { kind: "problem", key: `problem:${envelope.seq}`, message: event.message });
    // Shown elsewhere (the tile carries usage), or nowhere outside the
    // developer pane (passthrough), or implied by the items around them.
    case "turn.completed":
      // A turn that ended other than by finishing says so; one that
      // succeeded needs no mark, its last words are on the page.
      return event.outcome === "success"
        ? next
        : add(next, { kind: "notice", key: `turn:${envelope.seq}`, message: turnOutcome[event.outcome] ?? "The turn ended early." });
    case "session.started":
    case "turn.started":
    case "usage.updated":
    case "backend.passthrough":
      return next;
    // An event type this window does not know, under a version it does:
    // Core is newer in a way that kept `v`. Say so rather than drop it.
    default:
      return add(next, {
        kind: "notice",
        key: `unknown:${envelope.seq}`,
        message: `An event this window does not show (${String((event as { type?: unknown }).type)}).`,
      });
  }
}

/** Whether the request still waits for an answer. */
export function isOpen(item: Extract<Item, { kind: "permission" }>): boolean {
  return item.decision === undefined && !item.unanswered;
}

const endedReason: Partial<Record<string, string>> = {
  user_stopped: "Stopped.",
  backend_exited: "The agent exited.",
  error: "The session ended with an error.",
};

const turnOutcome: Partial<Record<string, string>> = {
  error: "The turn ended with an error.",
  interrupted: "The turn was interrupted.",
  max_turns: "The turn stopped at its turn limit.",
};

function add(transcript: Transcript, item: Item): Transcript {
  return { ...transcript, items: [...transcript.items, item] };
}

/** Replaces the item with the same key where it stands, or adds it at the end. */
function put(transcript: Transcript, item: Item): Transcript {
  const at = transcript.items.findIndex((i) => i.key === item.key);
  if (at < 0) return add(transcript, item);
  const items = transcript.items.slice();
  items[at] = item;
  return { ...transcript, items };
}

/** Changes the item with `key`, if there is one of `kind`; events about an unseen item change nothing. */
function patch<K extends Item["kind"]>(
  transcript: Transcript,
  key: string,
  kind: K,
  change: (item: Extract<Item, { kind: K }>) => Item,
): Transcript {
  const at = transcript.items.findIndex((i) => i.key === key);
  const item = transcript.items[at];
  if (!item || item.kind !== kind) return transcript;
  const items = transcript.items.slice();
  items[at] = change(item as Extract<Item, { kind: K }>);
  return { ...transcript, items };
}

/**
 * A tool's output as text: as it came; the text of its blocks when it is a
 * list of text blocks, as most tools' structured output is; else as JSON.
 */
function contentText(content: unknown): string {
  if (typeof content === "string") return content;
  const isText = (b: unknown): b is { type: "text"; text: string } =>
    typeof b === "object" && b !== null && (b as { type?: unknown }).type === "text" && typeof (b as { text?: unknown }).text === "string";
  if (Array.isArray(content) && content.length > 0 && content.every(isText)) return content.map((b) => b.text).join("\n");
  return JSON.stringify(content, null, 2);
}
