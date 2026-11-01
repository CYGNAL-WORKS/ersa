// Checks on what Core sends, at the boundary. The generated types say what
// Core should send; a newer, older or broken Core may send something else,
// and one malformed event must not take the window down (and, replayed from
// the start on every launch, keep it down). Events that fail are shown as a
// notice in the transcript; tiles and sessions are repaired to safe values.

import type { Session, Tile } from "../protocol/core-api.gen";

type Fields = Record<string, unknown>;

const isObject = (v: unknown): v is Fields => typeof v === "object" && v !== null && !Array.isArray(v);
const isString = (v: unknown): v is string => typeof v === "string";
const isNumber = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
const optional = (check: (v: unknown) => boolean) => (v: unknown) => v === undefined || check(v);

/** The fields each event type's rendering reads, and what they must be. Types not listed are rendered from no fields. */
const eventFields: Record<string, Record<string, (v: unknown) => boolean>> = {
  "session.state": { state: isString },
  "session.ended": { reason: isString },
  "session.notice": { message: isString },
  "turn.completed": { outcome: isString },
  "user.message": { text: isString },
  "assistant.text": { block_id: isString, text: isString, final: (v) => typeof v === "boolean" },
  "assistant.thinking": { block_id: isString, final: (v) => typeof v === "boolean", summary: optional(isString) },
  "tool.requested": { tool_use_id: isString, name: isString, display_name: optional(isString), input: isObject },
  "tool.progress": { tool_use_id: isString, detail: optional(isString) },
  "tool.result": { tool_use_id: isString, is_error: (v) => typeof v === "boolean", content: (v) => v !== undefined },
  "permission.requested": {
    request_id: isString,
    tool_use_id: isString,
    tool_name: isString,
    display_name: optional(isString),
    description: optional(isString),
    blocked_path: optional(isString),
    input: isObject,
  },
  "permission.decided": { request_id: isString, behavior: (v) => v === "allow" || v === "deny", decided_by: isString },
  "permission.denied_unanswered": { message: isString },
  "question.asked": {
    request_id: isString,
    questions: (v) => Array.isArray(v) && v.every((q) => isObject(q) && isString(q.id) && isString(q.text)),
  },
  "question.answered": { request_id: isString },
  error: { message: isString },
};

/** Whether `value` is an envelope this window can order: an object with a whole-number `seq`. */
export function hasSeq(value: unknown): value is { seq: number } {
  return isObject(value) && Number.isInteger(value.seq);
}

/**
 * Why `envelope` cannot be rendered, or `undefined` when it can. Only the
 * fields the window reads are checked; the rest pass through untouched.
 */
export function eventProblem(envelope: unknown): string | undefined {
  if (!isObject(envelope) || !isObject(envelope.event)) return "it has no event";
  const event = envelope.event;
  if (!isString(event.type)) return "its event has no type";
  const fields = eventFields[event.type];
  if (!fields) return undefined;
  const bad = Object.entries(fields).find(([key, check]) => !check(event[key]));
  return bad ? `${event.type} has an unreadable ${bad[0]}` : undefined;
}

/** A tile as the rail can show it: fields of the wrong kind are dropped, as if Core had not sent them. */
export function checkTile(value: unknown): Tile {
  const tile = isObject(value) ? value : {};
  const pending = Array.isArray(tile.pending)
    ? tile.pending.filter((p): p is Tile["pending"][number] => isObject(p) && isString(p.kind) && isString(p.request_id))
    : [];
  return {
    // An unreadable state reads as an error: the tile then says something is wrong.
    state: isString(tile.state) ? (tile.state as Tile["state"]) : "error",
    pending,
    ...(isNumber(tile.context_tokens) ? { context_tokens: tile.context_tokens } : {}),
    ...(isNumber(tile.context_window) ? { context_window: tile.context_window } : {}),
    ...(isNumber(tile.cost_usd_total) ? { cost_usd_total: tile.cost_usd_total } : {}),
  };
}

/** The sessions of a `session.list` answer the rail can show; entries without an id are left out. */
export function checkSessions(value: unknown): Session[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((s): Session[] => {
    if (!isObject(s) || !isString(s.id)) return [];
    return [
      {
        ...(s as unknown as Session),
        title: isString(s.title) ? s.title : undefined,
        created_at: isString(s.created_at) ? s.created_at : "",
        tile: checkTile(s.tile),
      },
    ];
  });
}
