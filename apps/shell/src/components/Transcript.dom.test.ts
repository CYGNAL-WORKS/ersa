// @vitest-environment jsdom
// The transcript's permission card in a DOM: where focus goes when the user answers.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { Envelope } from "../protocol/agent-events.gen";
import { EVENT_VERSION, apply, emptyTranscript } from "../session/transcript";
import { Transcript } from "./Transcript";

const requested: Envelope = {
  v: EVENT_VERSION,
  session_id: "s1",
  seq: 0,
  ts: "2026-11-06T09:00:00Z",
  source: { backend: "claude-code", backend_version: "2.1.302", input: "live" },
  event: {
    type: "permission.requested",
    request_id: "r1",
    tool_use_id: "t1",
    tool_name: "Write",
    input: { file_path: "notes.txt" },
    suggestions: [],
    flags: { default_to_no: false, suppress_always_allow: false, requires_user_interaction: false, classifier_approvable: false },
  },
};

afterEach(cleanup);

describe("the transcript's permission card", () => {
  it("keeps focus on the card when an answer makes its answers unavailable", () => {
    render(
      createElement(Transcript, {
        title: "Write the notes",
        view: { id: "s1", transcript: apply(emptyTranscript, requested), stream: { state: "live" } },
        onDecide: () => {},
        onAnswer: () => {},
      }),
    );
    const allow = screen.getByRole("button", { name: "Allow" });
    allow.focus();
    fireEvent.click(allow);
    expect(document.activeElement).toBe(screen.getByRole("region", { name: "Permission: Write" }));
  });
});
