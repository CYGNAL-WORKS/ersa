import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { SessionList } from "../components/SessionList";
import type { Envelope } from "../protocol/agent-events.gen";
import { apply, emptyTranscript } from "../session/transcript";
import { checkSessions, checkTile, eventProblem } from "./checks";

const envelope = (seq: number, event: unknown) =>
  ({
    v: 2,
    session_id: "s1",
    seq,
    ts: "2026-01-01T00:00:00Z",
    source: { backend: "claude-code", backend_version: "2.1.302", input: "live" },
    event,
  }) as unknown as Envelope;

describe("events from Core", () => {
  it("become a notice in their place when a field the window reads is the wrong kind", () => {
    const shown = [
      envelope(0, { type: "assistant.text", block_id: "b", text: 42, final: true }),
      envelope(1, { type: "question.asked", request_id: "q" }),
      envelope(2, { type: "permission.requested", request_id: "r", tool_use_id: "t", tool_name: "Bash", input: null }),
      envelope(3, { type: "user.message", text: "still here", origin: "human" }),
    ].reduce(apply, emptyTranscript);
    expect(shown.items.map((i) => i.kind)).toEqual(["notice", "notice", "notice", "user"]);
    expect(shown.items[0]).toMatchObject({ message: "An event this window could not read: assistant.text has an unreadable text." });
    expect(shown.lastSeq).toBe(3);
  });

  it("without a whole-number seq are not applied at all", () => {
    expect(apply(emptyTranscript, { seq: "0" } as unknown as Envelope)).toBe(emptyTranscript);
    expect(apply(emptyTranscript, null as unknown as Envelope)).toBe(emptyTranscript);
  });

  it("pass when only fields the window does not read are unusual", () => {
    expect(eventProblem(envelope(0, { type: "usage.updated", cost_usd_total: "a lot" }))).toBeUndefined();
  });
});

describe("tiles and sessions from Core", () => {
  it("drop fields of the wrong kind, so the rail still renders", () => {
    const tile = checkTile({ state: "running", cost_usd_total: "0.82", pending: "none" });
    expect(tile).toEqual({ state: "running", pending: [] });
    const sessions = checkSessions([{ id: "s1", created_at: 7, tile: { cost_usd_total: {}, pending: [{ kind: "permission" }] } }, { title: "no id" }]);
    expect(sessions).toHaveLength(1);
    expect(sessions[0]?.tile).toEqual({ state: "error", pending: [] });
    const html = renderToStaticMarkup(createElement(SessionList, { view: { state: "ready", sessions }, onSelect: () => {} }));
    expect(html).toContain("Untitled session");
  });

  it("that are not a list read as an empty list", () => {
    expect(checkSessions({ sessions: [] })).toEqual([]);
  });
});

describe("the error boundary", () => {
  it("shows what failed in place of the part that threw, and tries again for another key", () => {
    const boundary = new ErrorBoundary({ what: "This session", resetKey: "s1", children: "fine" });
    boundary.state = { ...ErrorBoundary.getDerivedStateFromError(new Error("cannot read tile")), key: "s1" };
    expect(renderToStaticMarkup(boundary.render() as never)).toContain("This session could not be shown: cannot read tile");
    expect(ErrorBoundary.getDerivedStateFromProps({ what: "x", resetKey: "s2", children: null }, boundary.state)).toEqual({
      error: undefined,
      key: "s2",
    });
  });
});
