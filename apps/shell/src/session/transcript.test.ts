// The transcript against the replay server's streams (issue #14, Test
// seams): streamed text grows in place and settles, a permission request is
// shown open until its decision, and only projected events change anything.

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Transcript as TranscriptView } from "../components/Transcript";
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
    expect(refused.refused).toMatch(/version 2/);
  });
});
