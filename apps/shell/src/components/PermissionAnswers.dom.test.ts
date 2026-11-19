// @vitest-environment jsdom
// A permission request's answers in a DOM: what each one decides.

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { Envelope, PermissionRequested } from "../protocol/agent-events.gen";
import type { Decision } from "../session/answers";
import { EVENT_VERSION, apply, emptyTranscript } from "../session/transcript";
import { PermissionAnswers } from "./PermissionAnswers";
import { Transcript } from "./Transcript";

const request: PermissionRequested = {
  type: "permission.requested",
  request_id: "r1",
  tool_use_id: "t1",
  tool_name: "Bash",
  input: { command: "pnpm test" },
  suggestions: [
    { kind: "allow_rule", label: "Allow pnpm commands", payload: {} },
    { kind: "set_mode", label: "Switch to acceptEdits mode", payload: {} },
  ],
  flags: { default_to_no: false, suppress_always_allow: false, requires_user_interaction: false, classifier_approvable: false },
};

afterEach(cleanup);

describe("a permission request's answers", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R13
  it("decides allow, deny, or allow with the chosen suggestion's index", () => {
    const decided: Decision[] = [];
    render(createElement(PermissionAnswers, { request, live: true, onDecide: (d: Decision) => decided.push(d) }));
    for (const name of ["Allow", "Deny", "Switch to acceptEdits mode", "Allow pnpm commands"]) {
      fireEvent.click(screen.getByRole("button", { name }));
    }
    expect(decided).toEqual(["allow", "deny", { suggestion: 1 }, { suggestion: 0 }]);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R12
  it("names each suggestion by its whole label, with no tooltip standing in for it", () => {
    const label = "Always allow Bash(echo probe *) in this project's local settings";
    const long = { ...request, suggestions: [{ kind: "allow_rule" as const, label, payload: {} }] };
    render(createElement(PermissionAnswers, { request: long, live: true, onDecide: () => {} }));
    const suggestion = screen.getByRole("button", { name: label });
    expect(suggestion.textContent).toBe(label);
    expect(suggestion.hasAttribute("title")).toBe(false);
  });
});

describe("an answer Ersa can't tell reached the agent, on the transcript's permission card", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R31
  it("isn't said to be refused: the card says Ersa can't tell, and offers the answers again", () => {
    const asked: Envelope = {
      v: EVENT_VERSION,
      session_id: "s1",
      seq: 0,
      ts: "2026-11-19T09:00:00Z",
      source: { backend: "claude-code", backend_version: "2.1.316", input: "live" },
      event: request,
    };
    const decided: [string, Decision][] = [];
    render(
      createElement(Transcript, {
        title: "Fix the build",
        view: { id: "s1", transcript: apply(emptyTranscript, asked), stream: { state: "live" } },
        answers: { r1: { state: "unknown" } },
        onDecide: (requestId: string, decision: Decision) => decided.push([requestId, decision]),
        onAnswer: () => {},
      }),
    );
    const card = screen.getByRole("region", { name: "Permission: Bash" });
    expect(within(card).queryByText("Your answer wasn't accepted.")).toBeNull();
    const note = within(card).getByText("Ersa can't tell whether your answer reached the agent. Answer again if this request is still waiting.").closest("p");
    expect(note?.getAttribute("role")).toBe("status");
    expect(note?.querySelector('.glyph[data-tone="caution"]')).toBeTruthy();
    for (const name of ["Allow", "Deny", "Allow pnpm commands"]) {
      expect((within(card).getByRole("button", { name }) as HTMLButtonElement).disabled).toBe(false);
    }
    fireEvent.click(within(card).getByRole("button", { name: "Deny" }));
    expect(decided).toEqual([["r1", "deny"]]);
  });
});

describe("an answer Ersa can't tell reached the agent, while Ersa can't reach Core", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R31
  it("says only that Ersa is reconnecting, with the answers unavailable", () => {
    render(createElement(PermissionAnswers, { request, answer: { state: "unknown" }, live: false, onDecide: () => {} }));
    expect(screen.queryByText("Ersa can't tell whether your answer reached the agent. Answer again if this request is still waiting.")).toBeNull();
    expect(screen.getByRole("status").textContent).toBe("Reconnecting to Core…");
    expect((screen.getByRole("button", { name: "Allow" }) as HTMLButtonElement).disabled).toBe(true);
  });
});
